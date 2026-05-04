//! `svelte/no-reactive-literals` — disallow assignments of literal values in reactive statements.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Expression, Statement};
use oxc::span::Span;

pub struct NoReactiveLiterals;

impl Rule for NoReactiveLiterals {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-literals"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // `meta.conditions` in vendor excludes Svelte 5 runes mode.
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
            // Only flag the simple form `$: var = literal;`.
            let Statement::ExpressionStatement(es) = &ls.body else {
                continue;
            };
            let Expression::AssignmentExpression(ae) = &es.expression else {
                continue;
            };
            if !is_literal_rhs(&ae.right) {
                continue;
            }
            // Vendor reports on the whole `SvelteReactiveStatement`.
            let s = content_offset + ls.span.start;
            let e = content_offset + ls.span.end;
            ctx.diagnostic(
                "Do not assign literal values inside reactive statements unless absolutely necessary.",
                Span::new(s, e),
            );
        }
    }
}

/// Mirrors vendor's selector: `Literal` (string / number / boolean / null /
/// bigint / regex) plus empty array-literal and empty object-literal.
/// Vendor does **not** match `TemplateLiteral`, the bare `undefined`
/// identifier, or `UnaryExpression` on a literal (`-1` is parsed as
/// `UnaryExpression`, not `Literal`).
fn is_literal_rhs(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => true,
        Expression::ArrayExpression(a) => a.elements.is_empty(),
        Expression::ObjectExpression(o) => o.properties.is_empty(),
        _ => false,
    }
}
