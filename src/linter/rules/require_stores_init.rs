//! `svelte/require-stores-init` — require store variables to be initialized.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{
    Argument, Expression, ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct RequireStoresInit;

struct StoreFactoryLocals {
    named: Vec<(String, &'static str)>,
    namespaces: Vec<String>,
}

impl Rule for RequireStoresInit {
    fn name(&self) -> &'static str {
        "svelte/require-stores-init"
    }

    fn applies_to_scripts(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(semantic) = ctx.instance_semantic {
            check_semantic(ctx, semantic, ctx.instance_content_offset);
        }
        if let Some(semantic) = ctx.module_semantic {
            check_semantic(ctx, semantic, ctx.module_content_offset);
        }
    }
}

fn check_semantic(
    ctx: &mut LintContext<'_>,
    semantic: &oxc::semantic::Semantic<'_>,
    content_offset: u32,
) {
    let factories = collect_store_factory_locals(semantic);
    if factories.named.is_empty() && factories.namespaces.is_empty() {
        return;
    }

    for node in semantic.nodes().iter() {
        let AstKind::CallExpression(ce) = node.kind() else {
            continue;
        };
        let Some(factory) = store_factory_call_name(&ce.callee, &factories) else {
            continue;
        };

        // A spread anywhere in the argument list disqualifies the check —
        // we can't statically tell how many values land in the call.
        if ce
            .arguments
            .iter()
            .any(|a| matches!(a, Argument::SpreadElement(_)))
        {
            continue;
        }

        let min_args = match factory {
            "writable" | "readable" => 1,
            "derived" => 3,
            _ => 0,
        };
        let should_report = ce.arguments.len() < min_args;

        if should_report {
            let s = content_offset + ce.span.start;
            let e = content_offset + ce.span.end;
            ctx.diagnostic(
                "Always set a default value for svelte stores.",
                Span::new(s, e),
            );
        }
    }
}

fn collect_store_factory_locals(semantic: &oxc::semantic::Semantic<'_>) -> StoreFactoryLocals {
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
                    let original = match imported {
                        "writable" => "writable",
                        "readable" => "readable",
                        "derived" => "derived",
                        _ => continue,
                    };
                    named.push((s.local.name.to_string(), original));
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

fn store_factory_call_name<'a>(
    callee: &'a Expression<'a>,
    factories: &'a StoreFactoryLocals,
) -> Option<&'static str> {
    match callee {
        Expression::Identifier(callee) => factories
            .named
            .iter()
            .find(|(local, _)| local == callee.name.as_str())
            .map(|(_, factory)| *factory),
        Expression::StaticMemberExpression(member) => {
            let Expression::Identifier(namespace) = &member.object else {
                return None;
            };
            if !factories
                .namespaces
                .iter()
                .any(|local| local == namespace.name.as_str())
            {
                return None;
            }
            match member.property.name.as_str() {
                "writable" => Some("writable"),
                "readable" => Some("readable"),
                "derived" => Some("derived"),
                _ => None,
            }
        }
        _ => None,
    }
}
