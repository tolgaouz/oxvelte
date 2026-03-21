//! `svelte/no-not-function-handler` — disallow non-function event handlers.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
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
                    if let Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } = attr {
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(eq_pos) = region.find('=') {
                            let value = region[eq_pos + 1..].trim();
                            if value.starts_with('{') && value.ends_with('}') {
                                let expr = value[1..value.len()-1].trim();
                                // Check for inline non-function literals
                                if is_non_function_literal(expr) {
                                    ctx.diagnostic(
                                        format!("Expected a function as event handler, got '{}'.", expr),
                                        *span,
                                    );
                                } else if expr.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                                    // Simple identifier reference — check if it's a known non-function var
                                    if non_fn_vars.contains(expr) {
                                        ctx.diagnostic(
                                            format!("Expected a function as event handler, got '{}'.", expr),
                                            *span,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
