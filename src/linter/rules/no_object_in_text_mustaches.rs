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
                        ctx.diagnostic(
                            format!("Unexpected {} in text mustache interpolation.", label),
                            tag.span,
                        );
                    }
                }
                TemplateNode::Element(el) => {
                    for attr in &el.attributes {
                        if let Attribute::NormalAttribute { value: AttributeValue::Concat(parts), span, .. } = attr {
                            for part in parts {
                                if let AttributeValuePart::Expression(expr) = part {
                                    let trimmed = expr.trim();
                                    if let Some(label) = detect_expression_kind(trimmed) {
                                        ctx.diagnostic(
                                            format!("Unexpected {} in text mustache interpolation.", label),
                                            *span,
                                        );
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

/// Detect if an expression is an object, array, function, arrow function, or class expression.
/// Returns a label string if it matches, None otherwise.
fn detect_expression_kind(expr: &str) -> Option<&'static str> {
    // Object literal: starts with `{`
    if expr.starts_with('{') {
        return Some("object");
    }
    // Array literal: starts with `[` — but only if it's a standalone array,
    // not an array used in a method call chain like `[...].includes(x)`.
    if expr.starts_with('[') {
        // Find the matching `]` then check if anything follows (method call = not a standalone array)
        if let Some(end) = find_matching(expr, '[', ']') {
            let after = expr[end + 1..].trim_start();
            if after.is_empty() {
                return Some("array");
            }
        } else {
            // No matching bracket found — treat as array literal
            return Some("array");
        }
    }
    // Arrow function: the expression itself must be an arrow function at the top level.
    // A top-level arrow starts with `(` (params) or an identifier followed by `=>`.
    // Expressions like `items.filter((x) => x.active)` are CallExpressions, not arrow functions.
    if is_top_level_arrow(expr) {
        return Some("function");
    }
    // Function expression
    if expr.starts_with("function") {
        let after = &expr["function".len()..];
        if after.is_empty() || after.starts_with(' ') || after.starts_with('(') || after.starts_with('*') {
            return Some("function");
        }
    }
    // Class expression
    if expr.starts_with("class") {
        let after = &expr["class".len()..];
        if after.is_empty() || after.starts_with(' ') || after.starts_with('{') {
            return Some("class");
        }
    }
    None
}

/// Check if the expression is a top-level arrow function expression.
/// Matches patterns like: `() => ...`, `(a, b) => ...`, `x => ...`, `async () => ...`
fn is_top_level_arrow(expr: &str) -> bool {
    let s = expr.trim();

    // Handle `async` prefix
    let s = if s.starts_with("async") {
        let after = &s["async".len()..];
        if after.starts_with(' ') || after.starts_with('(') {
            after.trim_start()
        } else {
            return false;
        }
    } else {
        s
    };

    if s.starts_with('(') {
        // Find matching `)` then check for `=>`
        if let Some(close) = find_matching(s, '(', ')') {
            let after = s[close + 1..].trim_start();
            return after.starts_with("=>");
        }
    } else {
        // Single-param arrow: `identifier => ...`
        // The identifier must be a simple name (alphanumeric/underscore)
        let end = s.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').unwrap_or(s.len());
        if end > 0 {
            let after = s[end..].trim_start();
            if after.starts_with("=>") {
                return true;
            }
        }
    }
    false
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
