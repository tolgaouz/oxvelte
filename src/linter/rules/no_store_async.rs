//! `svelte/no-store-async` — disallow async functions inside Svelte store
//! callbacks (they break auto-unsubscribe).
//!
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{
    Argument, Expression, ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc::ast::AstKind;
use oxc::semantic::Semantic;
use oxc::span::Span;

pub struct NoStoreAsync;

struct StoreFactoryLocals {
    named: Vec<String>,
    namespaces: Vec<String>,
}

/// `import { writable, readable as r, derived } from 'svelte/store'` and
/// `import * as stores from 'svelte/store'` → returns local bindings that can
/// resolve to one of the three store factories. Renamed imports are accepted
/// (`r` → `readable`); default imports are ignored.
fn collect_store_factory_locals<'a>(semantic: &Semantic<'a>) -> StoreFactoryLocals {
    let mut named = Vec::new();
    let mut namespaces = Vec::new();
    let program = semantic.nodes().program();
    for stmt in &program.body {
        let Statement::ImportDeclaration(imp) = stmt else {
            continue;
        };
        if imp.source.value != "svelte/store" {
            continue;
        }
        let Some(specifiers) = &imp.specifiers else {
            continue;
        };
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let imported = match &s.imported {
                        ModuleExportName::IdentifierName(n) => n.name.as_str(),
                        ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                        ModuleExportName::StringLiteral(l) => l.value.as_str(),
                    };
                    if is_store_factory_name(imported) {
                        named.push(s.local.name.to_string());
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    namespaces.push(s.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
            }
        }
    }
    StoreFactoryLocals { named, namespaces }
}

/// If `arg` is an async arrow- or function-expression, return its span.
fn async_callback_span(arg: &Argument<'_>) -> Option<Span> {
    match arg {
        Argument::ArrowFunctionExpression(a) if a.r#async => Some(a.span),
        Argument::FunctionExpression(f) if f.r#async => Some(f.span),
        _ => None,
    }
}

fn check_semantic<'a>(
    semantic: &Semantic<'a>,
    content_offset: u32,
    findings: &mut Vec<(String, Span)>,
) {
    let factory_locals = collect_store_factory_locals(semantic);
    if factory_locals.named.is_empty() && factory_locals.namespaces.is_empty() {
        return;
    }
    for node in semantic.nodes().iter() {
        let AstKind::CallExpression(ce) = node.kind() else {
            continue;
        };
        if !is_store_factory_call(&ce.callee, &factory_locals) {
            continue;
        }
        // Vendor inspects the 2nd argument: `writable(value, start?)`,
        // `readable(value, start?)`, `derived(stores, fn, initial?)`. All
        // three put the callback at index 1.
        let Some(arg) = ce.arguments.get(1) else {
            continue;
        };
        let Some(fn_span) = async_callback_span(arg) else {
            continue;
        };
        // Vendor narrows the report to the `async` keyword
        // (`start.column + 5`). The keyword sits at the start of the
        // function-expression / arrow-function span.
        let s = content_offset + fn_span.start;
        let e = s + 5;
        findings.push((
            "Do not pass async functions to svelte stores.".to_string(),
            Span::new(s, e),
        ));
    }
}

fn is_store_factory_name(name: &str) -> bool {
    matches!(name, "writable" | "readable" | "derived")
}

fn is_store_factory_call(callee: &Expression<'_>, locals: &StoreFactoryLocals) -> bool {
    match callee {
        Expression::Identifier(callee) => locals.named.iter().any(|l| l == callee.name.as_str()),
        Expression::StaticMemberExpression(member) => {
            let Expression::Identifier(namespace) = &member.object else {
                return false;
            };
            locals
                .namespaces
                .iter()
                .any(|l| l == namespace.name.as_str())
                && is_store_factory_name(member.property.name.as_str())
        }
        _ => false,
    }
}

impl Rule for NoStoreAsync {
    fn name(&self) -> &'static str {
        "svelte/no-store-async"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn applies_to_scripts(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let mut findings: Vec<(String, Span)> = Vec::new();
        if let Some(s) = ctx.instance_semantic {
            check_semantic(s, ctx.instance_content_offset, &mut findings);
        }
        if let Some(s) = ctx.module_semantic {
            check_semantic(s, ctx.module_content_offset, &mut findings);
        }
        for (msg, span) in findings {
            ctx.diagnostic(msg, span);
        }
    }
}
