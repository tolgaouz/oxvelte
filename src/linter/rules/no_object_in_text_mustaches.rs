//! `svelte/no-object-in-text-mustaches` — disallow objects in text mustache interpolation.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, TemplateNode};

pub struct NoObjectInTextMustaches;

impl Rule for NoObjectInTextMustaches {
    fn name(&self) -> &'static str {
        "svelte/no-object-in-text-mustaches"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            match node {
                TemplateNode::MustacheTag(tag) => {
                    let expr = tag.expression.trim();
                    let kind = detect_expression_kind(expr);
                    if let Some(label) = kind {
                        ctx.diagnostic(format!("Unexpected {} in text mustache interpolation.", label),
                            tag.span);
                    }
                }
                TemplateNode::Element(el) => {
                    for attr in &el.attributes {
                        if let Attribute::NormalAttribute { value: AttributeValue::Concat(parts), span, .. } = attr {
                            for part in parts {
                                if let AttributeValuePart::Expression(expr) = part {
                                    let trimmed = expr.trim();
                                    if let Some(label) = detect_expression_kind(trimmed) {
                                        ctx.diagnostic(format!("Unexpected {} in text mustache interpolation.", label),
                                            *span);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        });
    }
}

fn detect_expression_kind(expr: &str) -> Option<&'static str> {
    if expr.starts_with('{') {
        return Some("object");
    }
    if expr.starts_with('[') {
        if let Some(end) = find_matching(expr, '[', ']') {
            let after = expr[end + 1..].trim_start();
            if after.is_empty() {
                return Some("array");
            }
        } else {
            return Some("array");
        }
    }
    if is_top_level_arrow(expr) {
        return Some("function");
    }
    if expr.starts_with("function") {
        let after = &expr["function".len()..];
        if after.is_empty() || after.starts_with(' ') || after.starts_with('(') || after.starts_with('*') {
            return Some("function");
        }
    }
    if expr.starts_with("class") {
        let after = &expr["class".len()..];
        if after.is_empty() || after.starts_with(' ') || after.starts_with('{') {
            return Some("class");
        }
    }
    None
}

fn is_top_level_arrow(expr: &str) -> bool {
    let s = expr.trim();
    let s = match s.strip_prefix("async") {
        Some(after) if after.starts_with(' ') || after.starts_with('(') => after.trim_start(),
        Some(_) => return false,
        None => s,
    };
    if s.starts_with('(') {
        return find_matching(s, '(', ')').is_some_and(|close| s[close + 1..].trim_start().starts_with("=>"));
    }
    let end = s.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').unwrap_or(s.len());
    end > 0 && s[end..].trim_start().starts_with("=>")
}

fn find_matching(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = None::<char>;
    for (i, c) in s.char_indices() {
        match c {
            '\'' | '"' | '`' if in_string.is_none() => in_string = Some(c),
            c2 if in_string == Some(c2) => in_string = None,
            c2 if c2 == open && in_string.is_none() => depth += 1,
            c2 if c2 == close && in_string.is_none() => {
                depth -= 1;
                if depth == 0 { return Some(i); }
            }
            _ => {}
        }
    }
    None
}
