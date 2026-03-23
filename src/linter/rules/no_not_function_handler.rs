//! `svelte/no-not-function-handler` — disallow non-function event handlers.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use oxc::span::Span;
use std::collections::HashSet;

pub struct NoNotFunctionHandler;

/// Check if a JS expression string looks like a non-function literal.
fn is_non_function_literal(expr: &str) -> bool {
    let s = expr.trim();
    // Booleans, undefined
    if s == "true" || s == "false" || s == "undefined" {
        return true;
    }
    // Numbers (including negative)
    if s.parse::<f64>().is_ok() {
        return true;
    }
    // BigInt (e.g. 42n)
    if s.ends_with('n') && s[..s.len()-1].parse::<i64>().is_ok() {
        return true;
    }
    // String literals
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
        || (s.starts_with('`') && s.ends_with('`'))
    {
        return true;
    }
    // Regex
    if s.starts_with('/') && s.len() > 1 {
        // Find closing / (skip first char)
        if let Some(end) = s[1..].rfind('/') {
            let after = &s[end+2..];
            // After closing / should be just flags (gimsuvy)
            if after.chars().all(|c| "gimsuy".contains(c)) {
                return true;
            }
        }
    }
    // Array/Object literals
    if (s.starts_with('[') && s.ends_with(']'))
        || (s.starts_with('{') && s.ends_with('}'))
    {
        return true;
    }
    // class expressions, new expressions
    if s.starts_with("class ") || s.starts_with("new ") {
        return true;
    }
    false
}

/// Scan script content for variable declarations with non-function initializers.
/// Returns set of variable names that are known to be non-functions.
fn find_non_function_vars(content: &str) -> HashSet<String> {
    let mut non_fn_vars = HashSet::new();
    // Simple heuristic: look for `const|let|var NAME = <literal>;`
    for line in content.lines() {
        let trimmed = line.trim();
        for keyword in &["const ", "let ", "var "] {
            if let Some(rest) = trimmed.strip_prefix(keyword) {
                // Extract variable name (until = or space)
                let name_end = rest.find(|c: char| c == '=' || c == ' ' || c == ':').unwrap_or(rest.len());
                let name = rest[..name_end].trim();
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                    continue;
                }
                // Find the initializer after =
                if let Some(eq_pos) = rest.find('=') {
                    let init = rest[eq_pos + 1..].trim();
                    // Strip trailing semicolon
                    let init = init.strip_suffix(';').unwrap_or(init).trim();
                    if !init.is_empty() && is_non_function_literal(init) {
                        non_fn_vars.insert(name.to_string());
                    }
                }
            }
        }
    }
    non_fn_vars
}

/// Check if a handler attribute value contains a non-function expression.
fn check_handler_value(ctx: &mut LintContext, non_fn_vars: &HashSet<String>, span: Span) {
    let region = &ctx.source[span.start as usize..span.end as usize];
    if let Some(eq_pos) = region.find('=') {
        let value = region[eq_pos + 1..].trim();
        if value.starts_with('{') && value.ends_with('}') {
            let expr = value[1..value.len()-1].trim();
            let expr_start = span.start as usize + eq_pos + 1
                + region[eq_pos + 1..].len()
                - region[eq_pos + 1..].trim_start().len()
                + 1; // skip '{'
            // Find the actual position of expr within the braces
            let inner = &value[1..value.len()-1];
            let trim_offset = inner.len() - inner.trim_start().len();
            let expr_byte_start = (span.start as usize + eq_pos + 1
                + (value.as_ptr() as usize - region[eq_pos + 1..].trim().as_ptr() as usize).wrapping_add(0)
                ) as u32;
            let _ = expr_byte_start;
            let _ = expr_start;
            let _ = trim_offset;
            // For simplicity, compute expr span from the source
            let brace_open = span.start as usize + region.find('{').unwrap_or(eq_pos + 1);
            let expr_in_source = &ctx.source[brace_open + 1..span.end as usize];
            let trimmed_start = expr_in_source.len() - expr_in_source.trim_start().len();
            let expr_span_start = (brace_open + 1 + trimmed_start) as u32;
            let expr_span_end = expr_span_start + expr.len() as u32;
            let expr_span = Span::new(expr_span_start, expr_span_end);

            // Check for inline non-function literals
            if is_non_function_literal(expr) {
                let msg = if expr.starts_with('[') {
                    "Unexpected array in event handler.".to_string()
                } else if expr.starts_with('{') {
                    "Unexpected object in event handler.".to_string()
                } else if expr.starts_with('"') || expr.starts_with('\'') || expr.starts_with('`') {
                    "Unexpected string value in event handler.".to_string()
                } else if expr == "true" || expr == "false" {
                    "Unexpected boolean value in event handler.".to_string()
                } else if expr.starts_with("new ") {
                    "Unexpected new expression in event handler.".to_string()
                } else if expr.starts_with("class ") {
                    "Unexpected class expression in event handler.".to_string()
                } else {
                    format!("Expected a function as event handler, got '{}'.", expr)
                };
                ctx.diagnostic(msg, expr_span);
            } else if expr.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                // Simple identifier reference — check if it's a known non-function var
                if non_fn_vars.contains(expr) {
                    let msg = "Unexpected string value in event handler.".to_string();
                    ctx.diagnostic(msg, expr_span);
                }
            }
        }
    }
}

impl Rule for NoNotFunctionHandler {
    fn name(&self) -> &'static str {
        "svelte/no-not-function-handler"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Build set of non-function variable names from script
        let non_fn_vars = if let Some(script) = &ctx.ast.instance {
            find_non_function_vars(&script.content)
        } else {
            HashSet::new()
        };

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    // Check on:event directives (Svelte 4)
                    if let Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } = attr {
                        check_handler_value(ctx, &non_fn_vars, *span);
                    }
                    // Check Svelte 5 on* attributes (onclick, onmouseover, etc.)
                    if let Attribute::NormalAttribute { name, span, .. } = attr {
                        if name.starts_with("on") && name.len() > 2
                            && name.as_bytes()[2].is_ascii_lowercase()
                        {
                            check_handler_value(ctx, &non_fn_vars, *span);
                        }
                    }
                }
            }
        });
    }
}
