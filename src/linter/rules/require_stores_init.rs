//! `svelte/require-stores-init` — require store variables to be initialized.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Argument, Expression, ImportDeclarationSpecifier, ModuleExportName, Statement};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct RequireStoresInit;

impl Rule for RequireStoresInit {
    fn name(&self) -> &'static str {
        "svelte/require-stores-init"
    }

    fn applies_to_scripts(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        // Resolve imports of `writable`/`readable`/`derived` from `svelte/store`.
        // Value: the factory's original name.
        let mut factories: Vec<(String, &'static str)> = Vec::new();
        let program = semantic.nodes().program();
        for stmt in &program.body {
            let Statement::ImportDeclaration(imp) = stmt else { continue };
            if imp.source.value != "svelte/store" {
                continue;
            }
            let Some(specifiers) = &imp.specifiers else { continue };
            for spec in specifiers {
                if let ImportDeclarationSpecifier::ImportSpecifier(s) = spec {
                    let imported = match &s.imported {
                        ModuleExportName::IdentifierName(n) => n.name.as_str(),
                        ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                        ModuleExportName::StringLiteral(l) => l.value.as_str(),
                    };
                    let original: &'static str = match imported {
                        "writable" => "writable",
                        "readable" => "readable",
                        "derived" => "derived",
                        _ => continue,
                    };
                    factories.push((s.local.name.to_string(), original));
                }
            }
        }
        if factories.is_empty() {
            return;
        }

        for node in semantic.nodes().iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            let Expression::Identifier(callee) = &ce.callee else { continue };
            let Some((_, factory)) = factories.iter().find(|(local, _)| local == callee.name.as_str()) else { continue };

            // Spread-only first argument disqualifies the check (we can't reason).
            if let Some(Argument::SpreadElement(_)) = ce.arguments.first().map(|a| &*a) {
                continue;
            }

            let should_report = if *factory == "derived" {
                // `derived` requires at LEAST 3 args (stores, fn, initial). Vendor
                // flags calls with < 2 args.
                ce.arguments.is_empty() || ce.arguments.len() < 2
            } else {
                ce.arguments.is_empty()
            };

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
}
