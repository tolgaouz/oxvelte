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
            .map(|s| s != "always")
            .unwrap_or(true);

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

/// Returns true if the string is a valid single CSS class name (matches /^[\w-]+$/).
fn is_valid_class_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// Returns true if the string (contents of a quoted literal, already unquoted) is empty or
/// whitespace-only — i.e., effectively an empty class name.
fn is_empty_class_branch(s: &str) -> bool {
    s.trim().is_empty()
}

/// If `s` is a quoted string literal (single, double, or backtick without `${}`),
/// return its inner content. Otherwise return None.
fn unquote_str(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.len() >= 2 {
        let b = s.as_bytes();
        if (b[0] == b'\'' && b[s.len()-1] == b'\'') || (b[0] == b'"' && b[s.len()-1] == b'"') {
            return Some(&s[1..s.len()-1]);
        }
        // Backtick template literal with no interpolations
        if b[0] == b'`' && b[s.len()-1] == b'`' {
            let inner = &s[1..s.len()-1];
            if !inner.contains("${") {
                return Some(inner);
            }
        }
    }
    None
}

/// Split a ternary expression at the top-level `?` and `:` (depth 0).
/// Returns `(condition, true_branch, false_branch)` or `None` if not a ternary.
fn split_ternary(expr: &str) -> Option<(&str, &str, &str)> {
    let bytes = expr.as_bytes();
    let mut depth = 0i32;
    let mut q_pos = None;
    let mut c_pos = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'\'' | b'"' | b'`' => {
                // Skip string literal
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' { i += 1; }
                    else if bytes[i] == quote { break; }
                    i += 1;
                }
            }
            b'?' if depth == 0 && q_pos.is_none() => q_pos = Some(i),
            b':' if depth == 0 && q_pos.is_some() && c_pos.is_none() => c_pos = Some(i),
            _ => {}
        }
        i += 1;
    }
    if let (Some(q), Some(c)) = (q_pos, c_pos) {
        Some((
            expr[..q].trim(),
            expr[q+1..c].trim(),
            expr[c+1..].trim(),
        ))
    } else {
        None
    }
}

/// Check if an expression is a simple ternary like `cond ? 'class-name' : ''`
/// (either the true or false branch is empty/whitespace and the other is a valid class name).
fn is_simple_class_ternary(expr: &str) -> bool {
    let trimmed = expr.trim();
    let Some((_cond, true_branch, false_branch)) = split_ternary(trimmed) else {
        return false;
    };
    if let (Some(true_inner), Some(false_inner)) = (unquote_str(true_branch), unquote_str(false_branch)) {
        // false branch empty, true branch is a valid single class name
        if is_empty_class_branch(false_inner) && is_valid_class_name(true_inner.trim()) {
            return true;
        }
        // true branch empty, false branch is a valid single class name
        if is_empty_class_branch(true_inner) && is_valid_class_name(false_inner.trim()) {
            return true;
        }
    }
    false
}
