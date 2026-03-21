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
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            // Check text-level mustache tags
            if let TemplateNode::MustacheTag(tag) = node {
                check_expression(&tag.expression, tag.span, ctx);
            }
            // Check attribute-level mustache expressions
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { value, span, .. } = attr {
                        match value {
                            AttributeValue::Expression(expr) => {
                                check_expression(expr, *span, ctx);
                            }
                            AttributeValue::Concat(parts) => {
                                for part in parts {
                                    if let AttributeValuePart::Expression(expr) = part {
                                        // For concat parts we don't have individual spans,
                                        // so use the attribute span
                                        check_expression(expr, *span, ctx);
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

fn check_expression(expr: &str, span: oxc::span::Span, ctx: &mut LintContext<'_>) {
    let stripped = strip_leading_js_comments(expr.trim());
    let stripped = stripped.trim();
    if let Some(inner) = extract_simple_string_literal(stripped) {
        // Don't flag strings containing { or } — they can't be
        // used as raw text in Svelte templates.
        if inner.contains('{') || inner.contains('}') {
            return;
        }
        // Don't flag backtick strings with newlines
        if stripped.starts_with('`') && inner.contains('\n') {
            return;
        }
        ctx.diagnostic_with_fix(
            "Unnecessary mustache interpolation around a string literal. Use the text directly.",
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
