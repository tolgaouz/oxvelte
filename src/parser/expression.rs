//! On-demand typed parsing for mustache / attribute expression text.
//!
//! Our Svelte parser stores template expressions as opaque `String`s on
//! `MustacheTag`, `AttributeValue::Expression`, etc. — re-parsing them
//! through oxc is how rules get access to a real AST (`StringLiteral`,
//! `TemplateLiteral`, `UnaryExpression`, …) without hand-rolled tokenizers.
//!
//! The helpers here use a `void (…);` wrapper so:
//!   - any leading / trailing / inline comments inside the expression text
//!     surface on `ParserReturn.program.comments` (same shape ESLint's
//!     `sourceCode.getTokens({ includeComments: true })` gives vendor rules);
//!   - the expression sits inside the wrapper as a plain AST node, reachable
//!     via `unwrap_template_expression`.
//!
//! The wrapper source is copied into the caller-supplied `Allocator` so the
//! original text can be dropped immediately after this call returns.

use oxc::allocator::Allocator;
use oxc::ast::ast::{Expression, Statement};
use oxc::parser::{Parser, ParserReturn};
use oxc::span::SourceType;

/// Parse `text` as a single JavaScript expression wrapped in `void (…);`.
/// Uses TypeScript mode so TS syntax (`as`, satisfies, type assertions,
/// generics) inside template expressions parses cleanly.
pub fn parse_template_expression<'a>(text: &str, allocator: &'a Allocator) -> ParserReturn<'a> {
    let wrapper = format!("void ({});\n", text);
    let wrapped = allocator.alloc_str(&wrapper);
    Parser::new(allocator, wrapped, SourceType::ts()).parse()
}

/// Extract the inner expression from a `void (EXPR);` wrapper produced by
/// `parse_template_expression`. Returns `None` if the wrapper didn't parse
/// to the expected shape (e.g. the input wasn't a valid expression at all).
pub fn unwrap_template_expression<'a, 'r>(
    result: &'r ParserReturn<'a>,
) -> Option<&'r Expression<'a>> {
    let stmt = result.program.body.first()?;
    let Statement::ExpressionStatement(es) = stmt else { return None };
    let Expression::UnaryExpression(u) = &es.expression else { return None };
    Some(match &u.argument {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_literal() {
        let alloc = Allocator::default();
        let r = parse_template_expression("'foo'", &alloc);
        assert!(r.errors.is_empty());
        let expr = unwrap_template_expression(&r).unwrap();
        assert!(matches!(expr, Expression::StringLiteral(_)));
    }

    #[test]
    fn parses_template_literal() {
        let alloc = Allocator::default();
        let r = parse_template_expression("`foo`", &alloc);
        let expr = unwrap_template_expression(&r).unwrap();
        assert!(matches!(expr, Expression::TemplateLiteral(_)));
    }

    #[test]
    fn captures_comments() {
        let alloc = Allocator::default();
        let r = parse_template_expression("/* hi */ 'foo'", &alloc);
        assert_eq!(r.program.comments.len(), 1);
    }

    #[test]
    fn rejects_garbage() {
        let alloc = Allocator::default();
        let r = parse_template_expression("{{{", &alloc);
        // Parser will report errors; helper still returns a result.
        assert!(!r.errors.is_empty() || unwrap_template_expression(&r).is_none());
    }
}
