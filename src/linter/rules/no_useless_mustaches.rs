//! `svelte/no-useless-mustaches` — disallow unnecessary mustache interpolations.
//! ⭐ Recommended, 🔧 Fixable

use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};

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

fn check_attribute_value(value: &AttributeValue, span: oxc::span::Span, ctx: &mut LintContext<'_>, ignore_comment: bool, ignore_escape: bool) {
    match value {
        AttributeValue::Expression(expr) => check_expression(expr, span, ctx, ignore_comment, ignore_escape),
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

fn check_expression(expr: &str, span: oxc::span::Span, ctx: &mut LintContext<'_>, ignore_includes_comment: bool, ignore_string_escape: bool) {
    let trimmed = expr.trim();
    if ignore_includes_comment && (trimmed.contains("//") || trimmed.contains("/*")) {
        return;
    }
    let stripped = strip_leading_js_comments(trimmed);
    let stripped = stripped.trim();
    if let Some(inner) = extract_simple_string_literal(stripped) {
        if ignore_string_escape && inner.as_bytes().windows(2).any(|w| {
            w[0] == b'\\' && matches!(w[1], b'n' | b'r' | b'v' | b't' | b'b' | b'f' | b'u' | b'x')
        }) {
            return;
        }
        if inner.contains('{') {
            return;
        }
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

fn strip_leading_js_comments(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        if let Some(nl) = rest.starts_with("//").then(|| rest.find('\n')).flatten() {
            rest = rest[nl + 1..].trim_start();
        } else if let Some(end) = rest.starts_with("/*").then(|| rest.find("*/")).flatten() {
            rest = rest[end + 2..].trim_start();
        } else {
            return if rest.starts_with("//") || rest.starts_with("/*") { "" } else { rest };
        }
    }
}
