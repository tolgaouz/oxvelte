//! `svelte/no-useless-mustaches` — disallow unnecessary mustache interpolations.
//! ⭐ Recommended, 🔧 Fixable

use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, TemplateNode};

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
        // Config: { "ignoreIncludesComment": true, "ignoreStringEscape": true }
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let ignore_includes_comment = opts
            .and_then(|v| v.get("ignoreIncludesComment"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ignore_string_escape = opts
            .and_then(|v| v.get("ignoreStringEscape"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            // Check text-level mustache tags
            if let TemplateNode::MustacheTag(tag) = node {
                check_expression(&tag.expression, tag.span, ctx, ignore_includes_comment, ignore_string_escape);
            }
            // Check attribute-level mustache expressions
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { value, span, name, .. } = attr {
                        // Skip `this` attribute on `svelte:element` — mustaches are required there
                        if name == "this" && el.name.starts_with("svelte:") {
                            continue;
                        }
                        match value {
                            AttributeValue::Expression(expr) => {
                                check_expression(expr, *span, ctx, ignore_includes_comment, ignore_string_escape);
                            }
                            AttributeValue::Concat(parts) => {
                                for part in parts {
                                    if let AttributeValuePart::Expression(expr) = part {
                                        // For concat parts we don't have individual spans,
                                        // so use the attribute span
                                        check_expression(expr, *span, ctx, ignore_includes_comment, ignore_string_escape);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });
    }
}

fn check_expression(expr: &str, span: oxc::span::Span, ctx: &mut LintContext<'_>, ignore_includes_comment: bool, ignore_string_escape: bool) {
    let trimmed = expr.trim();
    // If ignoreIncludesComment is true, skip expressions containing JS comments
    if ignore_includes_comment && (trimmed.contains("//") || trimmed.contains("/*")) {
        return;
    }
    let stripped = strip_leading_js_comments(trimmed);
    let stripped = stripped.trim();
    if let Some(inner) = extract_simple_string_literal(stripped) {
        // If ignoreStringEscape is true, skip strings with meaningful control-character escapes
        // Only \n \r \v \t \b \f \u \x are "meaningful" — plain \\ and \' \" are not
        if ignore_string_escape {
            let bytes = inner.as_bytes();
            for i in 0..bytes.len().saturating_sub(1) {
                if bytes[i] == b'\\' {
                    let next = bytes[i + 1];
                    if matches!(next, b'n' | b'r' | b'v' | b't' | b'b' | b'f' | b'u' | b'x') {
                        return;
                    }
                }
            }
        }
        // Don't flag strings containing `{` — they can't be used as raw
        // text in Svelte templates (would start an expression block).
        // Note: `}` alone is fine; the vendor only skips `{`.
        if inner.contains('{') {
            return;
        }
        // Don't flag backtick strings with newlines
        if stripped.starts_with('`') && inner.contains('\n') {
            return;
        }
        ctx.diagnostic_with_fix(
            "Unexpected mustache interpolation with a string literal value.",
            span,
            Fix {
                span,
                replacement: inner.to_string(),
            },
        );
    }
}

/// If `s` is exactly a simple string literal (single/double quoted, or backtick without ${...}),
/// return the inner content. Otherwise return None.
fn extract_simple_string_literal(s: &str) -> Option<&str> {
    if s.len() < 2 { return None; }
    let quote = s.as_bytes()[0];
    if quote != b'\'' && quote != b'"' && quote != b'`' { return None; }
    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped char
            continue;
        }
        if quote == b'`' && bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            return None;
        }
        if bytes[i] == quote {
            let rest = &s[i + 1..];
            if rest.trim().is_empty() {
                return Some(&s[1..i]);
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Strip leading JS comments (// line comments and /* block comments */) from an expression.
fn strip_leading_js_comments(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        if rest.starts_with("//") {
            if let Some(nl) = rest.find('\n') {
                rest = rest[nl + 1..].trim_start();
            } else {
                return "";
            }
        } else if rest.starts_with("/*") {
            if let Some(end) = rest.find("*/") {
                rest = rest[end + 2..].trim_start();
            } else {
                return "";
            }
        } else {
            return rest;
        }
    }
}
