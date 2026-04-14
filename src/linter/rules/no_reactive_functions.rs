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
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        for stmt in &semantic.nodes().program().body {
            let Statement::LabeledStatement(ls) = stmt else { continue };
            if ls.label.name != "$" {
                continue;
            }
            // `$: name = <function expression>`
            let Statement::ExpressionStatement(es) = &ls.body else { continue };
            let Expression::AssignmentExpression(ae) = &es.expression else { continue };
            let is_fn = matches!(
                &ae.right,
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
            );
            if !is_fn {
                continue;
            }
            let s = content_offset + ls.label.span.start;
            let e = content_offset + ls.label.span.end + 1; // include `:`
            ctx.diagnostic(
                "Do not create functions inside reactive statements unless absolutely necessary.",
                Span::new(s, e),
            );
        }
    }
}
