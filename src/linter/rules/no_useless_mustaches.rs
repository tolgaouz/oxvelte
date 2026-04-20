//! `svelte/no-useless-mustaches` — disallow unnecessary mustache interpolations.
//! ⭐ Recommended, 🔧 Fixable
//!
//! Vendor reference: `vendor/eslint-plugin-svelte/.../src/rules/no-useless-mustaches.ts`.
//! Its implementation is a ~10-line type guard over a pre-parsed
//! `ESTree.Expression` (matching `Literal` with string value, or
//! `TemplateLiteral` with zero interpolations), plus comment/escape gates
//! driven by the `ignoreIncludesComment` and `ignoreStringEscape` options.
//!
//! Ours now mirrors that shape: each mustache / attribute expression is
//! parsed on demand via `crate::parser::expression::parse_template_expression`,
//! and the trivial-literal check is a type match on oxc's
//! `Expression::StringLiteral` / `Expression::TemplateLiteral`. The previous
//! hand-rolled byte tokenizer (`extract_simple_string_literal`,
//! `strip_leading_js_comments`, escape substring search) is gone.

use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};
use crate::parser::expression::{parse_template_expression, unwrap_template_expression};
use oxc::allocator::Allocator;
use oxc::ast::ast::Expression;
use oxc::span::Span;

pub struct NoUselessMustaches;

impl Rule for NoUselessMustaches {
    fn name(&self) -> &'static str {
        "svelte/no-useless-mustaches"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let get_bool = |key| opts.and_then(|v| v.get(key)).and_then(|v| v.as_bool()).unwrap_or(false);
        let ignore_comment = get_bool("ignoreIncludesComment");
        let ignore_escape = get_bool("ignoreStringEscape");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::MustacheTag(tag) = node {
                check_expression(&tag.expression, tag.span, ctx, ignore_comment, ignore_escape);
            }
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { value, span, name, .. } => {
                            if name == "this" && el.name.starts_with("svelte:") { continue; }
                            check_attribute_value(value, *span, ctx, ignore_comment, ignore_escape);
                        }
                        Attribute::Directive { kind: DirectiveKind::StyleDirective, value, span, .. } => {
                            check_attribute_value(value, *span, ctx, ignore_comment, ignore_escape);
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

fn check_attribute_value(
    value: &AttributeValue,
    span: Span,
    ctx: &mut LintContext<'_>,
    ignore_comment: bool,
    ignore_escape: bool,
) {
    match value {
        AttributeValue::Expression(expr) =>
            check_expression(expr, span, ctx, ignore_comment, ignore_escape),
        AttributeValue::Concat(parts) => {
            for part in parts {
                if let AttributeValuePart::Expression(expr) = part {
                    check_expression(expr, span, ctx, ignore_comment, ignore_escape);
                }
            }
        }
        _ => {}
    }
}

fn check_expression(
    expr_text: &str,
    diag_span: Span,
    ctx: &mut LintContext<'_>,
    ignore_comment: bool,
    ignore_escape: bool,
) {
    let alloc = Allocator::default();
    let result = parse_template_expression(expr_text, &alloc);
    if !result.errors.is_empty() { return; }
    let Some(expr) = unwrap_template_expression(&result) else { return };

    if ignore_comment && !result.program.comments.is_empty() { return; }

    // Only `Literal (string)` and `TemplateLiteral` with no interpolations
    // qualify — every other expression (identifiers, binary ops, numbers,
    // template literals with expressions) carries meaning and isn't
    // "useless".
    let raw = match expr {
        Expression::StringLiteral(lit) => {
            let raw_with_quotes = lit.raw.as_ref().map(|a| a.as_str()).unwrap_or("");
            if raw_with_quotes.len() < 2 { return; }
            // Vendor's `sourceCode.getText(expression).slice(1, -1)`: the
            // content between the opening and closing quote characters.
            &raw_with_quotes[1..raw_with_quotes.len() - 1]
        }
        Expression::TemplateLiteral(tl) => {
            if !tl.expressions.is_empty() { return; }
            let Some(quasi) = tl.quasis.first() else { return };
            let raw = quasi.value.raw.as_str();
            // Multi-line backtick templates are load-bearing; don't inline
            // them. This matches `valid-test02-input.svelte`.
            if raw.contains('\n') { return; }
            raw
        }
        _ => return,
    };

    // `{'{foo'}` and friends can't be unwrapped without clashing with
    // mustache-delimiter syntax. (Vendor's line 87.)
    if raw.contains('{') { return; }

    if ignore_escape && has_useful_escape(raw) { return; }

    ctx.diagnostic_with_fix(
        "Unexpected mustache interpolation with a string literal value.",
        diag_span,
        Fix { span: diag_span, replacement: raw.to_string() },
    );
}

/// Vendor `no-useless-mustaches.ts:91–114`: an escape is "useful" iff it
/// produces a different cooked character — one of `\n \r \v \t \b \f \u \x`.
/// Cosmetic escapes like `\\`, `\'`, `\"`, `\$` don't change meaning, so
/// unwrapping them is safe even when `ignoreStringEscape` is on.
fn has_useful_escape(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if matches!(bytes[i + 1], b'n' | b'r' | b'v' | b't' | b'b' | b'f' | b'u' | b'x') {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}
