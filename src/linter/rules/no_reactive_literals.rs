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
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        for stmt in &semantic.nodes().program().body {
            let Statement::LabeledStatement(ls) = stmt else { continue };
            if ls.label.name != "$" {
                continue;
            }
            // Only flag the simple form `$: var = literal;` — not blocks, not
            // computed RHS expressions.
            let Statement::ExpressionStatement(es) = &ls.body else { continue };
            let Expression::AssignmentExpression(ae) = &es.expression else { continue };
            if !is_literal_rhs(&ae.right) {
                continue;
            }
            let s = content_offset + ls.label.span.start;
            let e = content_offset + ls.label.span.end + 1; // include ':'
            ctx.diagnostic(
                "Do not assign literal values inside reactive statements unless absolutely necessary.",
                Span::new(s, e),
            );
        }
    }
}

/// Is this expression a "literal" for the purposes of this rule?
/// Matches: strings, numbers, booleans, null, `undefined`, empty arrays/objects,
/// and template literals without interpolation.
fn is_literal_rhs(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::ArrayExpression(a) => a.elements.is_empty(),
        Expression::ObjectExpression(o) => o.properties.is_empty(),
        // `-1`, `+5`, `!true` on literals.
        Expression::UnaryExpression(u) => is_literal_rhs(&u.argument),
        _ => false,
    }
}
