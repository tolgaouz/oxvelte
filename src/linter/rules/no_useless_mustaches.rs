//! `svelte/no-useless-mustaches` — disallow unnecessary mustache interpolations.
//! ⭐ Recommended, 🔧 Fixable
//!
//! Vendor reference: `vendor/eslint-plugin-svelte/.../src/rules/no-useless-mustaches.ts`.
//! Its implementation is a ~10-line type guard over a pre-parsed
//! `ESTree.Expression` (matching `Literal` with string value, or
//! `TemplateLiteral` with zero interpolations), plus comment/escape gates
//! driven by the `ignoreIncludesComment` and `ignoreStringEscape` options.
//!
//! Our `MustacheTag` now carries `expression_ast: Option<&Expression<'a>>`
//! pre-parsed by the template parser into the shared allocator. This rule
//! reads that AST directly for mustache-tag checks — no on-demand re-parse.
//! Attribute-value expressions (`value={…}`, `style:key={…}`) still go
//! through `parse_template_expression` for now; extending the AST to carry
//! typed expressions on `AttributeValue::Expression` is a follow-on cycle.
//!
//! For `ignoreIncludesComment`, we still call `parse_template_expression`
//! once per mustache to get `ParserReturn.program.comments` — oxc's
//! `parse_expression` alone doesn't surface comment trivia.

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
                check_mustache_tag(tag, ctx, ignore_comment, ignore_escape);
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

fn check_mustache_tag<'a>(
    tag: &crate::ast::MustacheTag<'a>,
    ctx: &mut LintContext<'_>,
    ignore_comment: bool,
    ignore_escape: bool,
) {
    // Typed AST path: read the pre-parsed Expression from the template AST.
    let Some(expr) = tag.expression_ast else {
        // Parser couldn't parse this expression — nothing to simplify.
        return;
    };
    let raw = match trivial_string_raw(expr) {
        Some(r) => r,
        None => return,
    };
    // `{'{foo'}` / `` {`foo\nbar`} `` cases (vendor lines 83, 87).
    if raw.contains('{') { return; }
    if is_template_literal(expr) && raw.contains('\n') { return; }

    // Comment detection needs a second parse (through the void(...) wrapper)
    // because `parse_expression` alone doesn't surface comments.
    if ignore_comment {
        let alloc = Allocator::default();
        let result = parse_template_expression(&tag.expression, &alloc);
        if !result.program.comments.is_empty() { return; }
    }

    if ignore_escape && has_useful_escape(raw) { return; }

    ctx.diagnostic_with_fix(
        "Unexpected mustache interpolation with a string literal value.",
        tag.span,
        Fix { span: tag.span, replacement: raw.to_string() },
    );
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
            check_attribute_expression(expr, span, ctx, ignore_comment, ignore_escape),
        AttributeValue::Concat(parts) => {
            for part in parts {
                if let AttributeValuePart::Expression(expr) = part {
                    check_attribute_expression(expr, span, ctx, ignore_comment, ignore_escape);
                }
            }
        }
        _ => {}
    }
}

/// Attribute-value path: re-parses the expression text through the shared
/// wrapper helper. Will migrate to the typed AST once
/// `AttributeValue::Expression` carries a pre-parsed `Expression<'a>`.
fn check_attribute_expression(
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

    let Some(raw) = trivial_string_raw(expr) else { return };
    if raw.contains('{') { return; }
    if is_template_literal(expr) && raw.contains('\n') { return; }
    if ignore_escape && has_useful_escape(raw) { return; }

    ctx.diagnostic_with_fix(
        "Unexpected mustache interpolation with a string literal value.",
        diag_span,
        Fix { span: diag_span, replacement: raw.to_string() },
    );
}

/// Match vendor's type guard: return the between-quotes raw string when
/// the expression is either a string `Literal` or a `TemplateLiteral` with
/// zero interpolations. Other expression shapes carry meaning — don't flag.
fn trivial_string_raw<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::StringLiteral(lit) => {
            let raw_with_quotes = lit.raw.as_ref().map(|a| a.as_str()).unwrap_or("");
            if raw_with_quotes.len() < 2 { return None; }
            Some(&raw_with_quotes[1..raw_with_quotes.len() - 1])
        }
        Expression::TemplateLiteral(tl) => {
            if !tl.expressions.is_empty() { return None; }
            let quasi = tl.quasis.first()?;
            Some(quasi.value.raw.as_str())
        }
        _ => None,
    }
}

fn is_template_literal(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::TemplateLiteral(_))
}

/// Vendor `no-useless-mustaches.ts:91–114`: an escape is "useful" iff it
/// maps to a different cooked character — one of `\n \r \v \t \b \f \u \x`.
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
