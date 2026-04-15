//! `svelte/require-event-dispatcher-types` — require type parameters for createEventDispatcher.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Expression, ImportDeclarationSpecifier, ModuleExportName, Statement};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct RequireEventDispatcherTypes;

impl Rule for RequireEventDispatcherTypes {
    fn name(&self) -> &'static str {
        "svelte/require-event-dispatcher-types"
    }

    fn is_recommended(&self) -> bool {
        // The vendor rule is gated to svelteVersions: ['3/4'].
        // createEventDispatcher is deprecated in Svelte 5, so this rule adds noise
        // in Svelte 5 projects. Disable by default (opt-in).
        false
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        if script.lang.as_deref() != Some("ts") {
            return;
        }
        let Some(semantic) = ctx.instance_semantic else { return };

        // Collect local names bound to `createEventDispatcher` from `svelte`.
        let mut names: Vec<String> = Vec::new();
        let program = semantic.nodes().program();
        for stmt in &program.body {
            let Statement::ImportDeclaration(imp) = stmt else { continue };
            if imp.source.value != "svelte" {
                continue;
            }
            let Some(specifiers) = &imp.specifiers else { continue };
            for spec in specifiers {
                match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        let imported = match &s.imported {
                            ModuleExportName::IdentifierName(n) => n.name.as_str(),
                            ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                            ModuleExportName::StringLiteral(l) => l.value.as_str(),
                        };
                        if imported == "createEventDispatcher" {
                            names.push(s.local.name.to_string());
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        names.push(format!("{}.createEventDispatcher", s.local.name));
                    }
                    _ => {}
                }
            }
        }
        if names.is_empty() {
            return;
        }

        let content_offset = ctx.instance_content_offset;
        for node in semantic.nodes().iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            // Missing type parameters → `.type_arguments` is None.
            if ce.type_arguments.is_some() {
                continue;
            }
            let Some(callee_text) = callee_static_name(&ce.callee) else { continue };
            if !names.iter().any(|n| n == &callee_text) {
                continue;
            }
            let s = content_offset + ce.span.start;
            let e = content_offset + ce.span.end;
            ctx.diagnostic(
                "Type parameters missing for the `createEventDispatcher` function call.",
                Span::new(s, e),
            );
        }
    }
}

fn callee_static_name(callee: &Expression<'_>) -> Option<String> {
    match callee {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::StaticMemberExpression(mem) => {
            if let Expression::Identifier(id) = &mem.object {
                Some(format!("{}.{}", id.name, mem.property.name))
            } else {
                None
            }
        }
        _ => None,
    }
}
