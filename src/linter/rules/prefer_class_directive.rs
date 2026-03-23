//! `svelte/prefer-class-directive` — prefer class directives over ternary class attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart};

pub struct PreferClassDirective;

impl Rule for PreferClassDirective {
    fn name(&self) -> &'static str {
        "svelte/prefer-class-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Config: { "prefer": "empty" } — only flag ternaries with empty false branch
        let prefer_empty = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("prefer"))
            .and_then(|v| v.as_str())
            .map(|s| s == "empty")
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Skip components (uppercase) but allow svelte:element
                let first_char = el.name.chars().next().unwrap_or('a');
                if first_char.is_uppercase() { return; }
                if el.name.starts_with("svelte:") && el.name != "svelte:element" { return; }

                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span } = attr {
                        if name == "class" {
                            match value {
                                AttributeValue::Expression(expr) => {
                                    if is_simple_class_ternary(expr) || (!prefer_empty && is_dual_class_ternary(expr)) {
                                        ctx.diagnostic(
                                            "Unexpected class using the ternary operator.",
                                            *span,
                                        );
                                    }
                                }
                                AttributeValue::Concat(parts) => {
                                    for (i, part) in parts.iter().enumerate() {
                                        if let AttributeValuePart::Expression(expr) = part {
                                            // Check for simple ternary with empty false branch
                                            if is_simple_class_ternary(expr) {
                                                let prev_ok = if i > 0 {
                                                    match &parts[i - 1] {
                                                        AttributeValuePart::Static(s) => s.is_empty() || s.ends_with(' '),
                                                        AttributeValuePart::Expression(_) => false,
                                                    }
                                                } else { true };
                                                let next_ok = if i + 1 < parts.len() {
                                                    match &parts[i + 1] {
                                                        AttributeValuePart::Static(s) => s.is_empty() || s.starts_with(' '),
                                                        AttributeValuePart::Expression(_) => false,
                                                    }
                                                } else { true };
                                                if prev_ok && next_ok {
                                                    ctx.diagnostic(
                                                        "Unexpected class using the ternary operator.",
                                                        *span,
                                                    );
                                                }
                                            }
                                            // Check dual-class ternary only if ALL other expression parts
                                            // have empty-string false branches (not spaces)
                                            else if !prefer_empty && is_dual_class_ternary(expr) {
                                                let all_others_empty = parts.iter().enumerate().all(|(j, p)| {
                                                    if j == i { return true; }
                                                    match p {
                                                        AttributeValuePart::Expression(e) => {
                                                            e.trim().ends_with(": ''") || e.trim().ends_with(": \"\"")
                                                                || !e.contains('?') // non-ternary expr
                                                        }
                                                        AttributeValuePart::Static(_) => true,
                                                    }
                                                });
                                                if all_others_empty {
                                                    ctx.diagnostic(
                                                        "Unexpected class using the ternary operator.",
                                                        *span,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        });
    }
}

/// Check if an expression is a ternary with two non-empty single-word class name branches.
/// e.g. `c ? 'active' : 'inactive'` can be `class:active={c} class:inactive={!c}`
fn is_dual_class_ternary(expr: &str) -> bool {
    let trimmed = expr.trim();
    if !trimmed.contains('?') || !trimmed.contains(':') { return false; }
    // Find ? and : at depth 0
    let mut depth = 0;
    let mut q_pos = None;
    for (i, c) in trimmed.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '?' if depth == 0 && q_pos.is_none() => q_pos = Some(i),
            _ => {}
        }
    }
    let q_pos = match q_pos { Some(p) => p, None => return false };
    let branches = &trimmed[q_pos + 1..];
    // Find colon at depth 0
    let mut depth = 0;
    let mut c_pos = None;
    for (i, c) in branches.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => { c_pos = Some(i); break; }
            _ => {}
        }
    }
    let c_pos = match c_pos { Some(p) => p, None => return false };
    let true_branch = branches[..c_pos].trim();
    let false_branch = branches[c_pos + 1..].trim();
    // Both branches must be non-empty single-word string literals (not just whitespace)
    is_single_class_name(true_branch) && is_single_class_name(false_branch)
}

fn is_single_class_name(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 3 { return false; }
    let inner = if (s.starts_with('\'') && s.ends_with('\''))
        || (s.starts_with('"') && s.ends_with('"')) {
        &s[1..s.len()-1]
    } else { return false; };
    let trimmed = inner.trim();
    !trimmed.is_empty() && !trimmed.contains(' ')
}

/// Check if an expression is a simple ternary like `cond ? 'class-name' : ''`
fn is_simple_class_ternary(expr: &str) -> bool {
    let trimmed = expr.trim();
    if !trimmed.contains('?') || !trimmed.contains(':') {
        return false;
    }
    trimmed.ends_with(": ''")
        || trimmed.ends_with(": \"\"")
        || trimmed.starts_with("'' :")
        || trimmed.starts_with("\"\" :")
}
