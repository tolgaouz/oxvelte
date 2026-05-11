//! `svelte/no-svelte-internal` — disallow importing from svelte/internal.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Expression, Statement};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct NoSvelteInternal;

impl Rule for NoSvelteInternal {
    fn name(&self) -> &'static str {
        "svelte/no-svelte-internal"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for (sem, offset) in [
            (ctx.instance_semantic, ctx.instance_content_offset),
            (ctx.module_semantic, ctx.module_content_offset),
        ]
        .into_iter()
        .filter_map(|(s, o)| s.map(|s| (s, o)))
        {
            for stmt in &sem.nodes().program().body {
                let source_span = match stmt {
                    Statement::ImportDeclaration(imp) => {
                        if is_svelte_internal(imp.source.value.as_str()) {
                            Some(imp.source.span)
                        } else {
                            None
                        }
                    }
                    Statement::ExportAllDeclaration(exp) => {
                        if is_svelte_internal(exp.source.value.as_str()) {
                            Some(exp.source.span)
                        } else {
                            None
                        }
                    }
                    Statement::ExportNamedDeclaration(exp) => exp.source.as_ref().and_then(|s| {
                        if is_svelte_internal(s.value.as_str()) {
                            Some(s.span)
                        } else {
                            None
                        }
                    }),
                    _ => None,
                };
                if let Some(span) = source_span {
                    // Report the inside of the string literal (between the quotes).
                    let s = offset + span.start + 1;
                    let e = offset + span.end - 1;
                    ctx.diagnostic(
                        "Using svelte/internal is prohibited. This will be removed in Svelte 6.",
                        Span::new(s, e),
                    );
                }
            }

            for node in sem.nodes().iter() {
                let AstKind::ImportExpression(import_expr) = node.kind() else {
                    continue;
                };
                let Expression::StringLiteral(lit) = &import_expr.source else {
                    continue;
                };
                if !is_svelte_internal(lit.value.as_str()) {
                    continue;
                }
                let s = offset + lit.span.start + 1;
                let e = offset + lit.span.end - 1;
                ctx.diagnostic(
                    "Using svelte/internal is prohibited. This will be removed in Svelte 6.",
                    Span::new(s, e),
                );
            }
        }
    }
}

fn is_svelte_internal(s: &str) -> bool {
    s == "svelte/internal" || s.starts_with("svelte/internal/")
}
