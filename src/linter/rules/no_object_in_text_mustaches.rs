//! `svelte/no-object-in-text-mustaches` — disallow objects in text mustache interpolation.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

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
            if let TemplateNode::MustacheTag(tag) = node {
                let expr = tag.expression.trim();
                let kind = detect_expression_kind(expr);
                if let Some(label) = kind {
                    ctx.diagnostic(
                        format!("Unexpected {} in text mustache interpolation.", label),
                        tag.span,
                    );
                }
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
    // Array literal: starts with `[`
    if expr.starts_with('[') {
        return Some("array");
    }
    // Arrow function: contains `=>` (covers `() => ...`, `x => ...`)
    if expr.contains("=>") {
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
