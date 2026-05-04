//! `svelte/no-reactive-functions` — disallow assigning functions to reactive declarations.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Expression, Statement};
use oxc::span::Span;

pub struct NoReactiveFunctions;

impl Rule for NoReactiveFunctions {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-functions"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Vendor's `meta.conditions` excludes Svelte 5 runes mode — `$:` has
        // different semantics there.
        if ctx.is_runes {
            return;
        }
        let Some(semantic) = ctx.instance_semantic else {
            return;
        };
        let content_offset = ctx.instance_content_offset;

        for stmt in &semantic.nodes().program().body {
            let Statement::LabeledStatement(ls) = stmt else {
                continue;
            };
            if ls.label.name != "$" {
                continue;
            }
            // `$: name = <function expression>`
            let Statement::ExpressionStatement(es) = &ls.body else {
                continue;
            };
            let Expression::AssignmentExpression(ae) = &es.expression else {
                continue;
            };
            let is_fn = matches!(
                &ae.right,
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
            );
            if !is_fn {
                continue;
            }
            // Vendor reports on the whole `SvelteReactiveStatement`, i.e. the
            // entire `$: name = (...) => {...};`.
            let s = content_offset + ls.span.start;
            let e = content_offset + ls.span.end;
            ctx.diagnostic(
                "Do not create functions inside reactive statements unless absolutely necessary.",
                Span::new(s, e),
            );
        }
    }
}
