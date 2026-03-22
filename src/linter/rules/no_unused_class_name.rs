//! `svelte/no-unused-class-name` — disallow class names in the template that are not
//! defined in the `<style>` block.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, DirectiveKind};
use std::collections::HashSet;
pub struct NoUnusedClassName;

impl Rule for NoUnusedClassName {
    fn name(&self) -> &'static str {
        "svelte/no-unused-class-name"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Config: { "allowedClassNames": ["name", "/^pattern$/"] }
        let allowed_class_names: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("allowedClassNames"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        // Separate plain names and regex patterns
        let mut allowed_plain: HashSet<String> = HashSet::new();
        let mut allowed_patterns: Vec<String> = Vec::new();
        for name in &allowed_class_names {
            if name.starts_with('/') && name.ends_with('/') && name.len() > 2 {
                allowed_patterns.push(name[1..name.len()-1].to_string());
            } else {
                allowed_plain.insert(name.clone());
            }
        }

        // Step 1: Extract all class selectors from the CSS content
        let mut css_classes = HashSet::new();
        if let Some(style) = &ctx.ast.css {
            let css = &style.content;
            let bytes = css.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'.' {
                    let start = i + 1;
                    let mut end = start;
                    while end < bytes.len()
                        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b'_')
                    {
                        end += 1;
                    }
                    if end > start {
                        let class_name = &css[start..end];
                        css_classes.insert(class_name.to_string());
                    }
                    i = end;
                } else {
                    i += 1;
                }
            }
        }

        // Step 2: Collect all template classes and check if they're defined in CSS
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let mut element_classes = Vec::new();

                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name, value, .. } if name == "class" => {
                            if let AttributeValue::Static(val) = value {
                                for cls in val.split_whitespace() {
                                    if !cls.is_empty() {
                                        element_classes.push(cls.to_string());
                                    }
                                }
                            }
                        }
                        Attribute::Directive { kind: DirectiveKind::Class, name: cls_name, .. } => {
                            element_classes.push(cls_name.clone());
                        }
                        _ => {}
                    }
                }

                // Report template classes not found in CSS
                for cls in &element_classes {
                    if !css_classes.contains(cls.as_str()) {
                        // Check if class is in allowed list
                        if allowed_plain.contains(cls.as_str()) {
                            continue;
                        }
                        if allowed_patterns.iter().any(|p| simple_regex_match(p, cls)) {
                            continue;
                        }
                        ctx.diagnostic(
                            format!("Unused class \"{}\".", cls),
                            el.span,
                        );
                    }
                }
            }
        });
    }
}

/// Simple regex-like matching for common patterns used in allowedClassNames.
/// Handles: ^prefix, suffix$, ^exact$, \d, \d{N,M}, character classes, etc.
fn simple_regex_match(pattern: &str, text: &str) -> bool {
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$');
    let inner = pattern
        .strip_prefix('^').unwrap_or(pattern)
        .strip_suffix('$').unwrap_or_else(|| pattern.strip_prefix('^').unwrap_or(pattern));

    // For fully anchored patterns (^...$), do a full match
    if anchored_start && anchored_end {
        return regex_match_inner(inner, text, 0, 0);
    }
    if anchored_start {
        return regex_match_inner(inner, text, 0, 0);
    }
    if anchored_end {
        // Try matching from each position
        for i in 0..=text.len() {
            if regex_match_inner(inner, text, 0, i) {
                return true;
            }
        }
        return false;
    }
    // Unanchored: contains match
    for i in 0..=text.len() {
        if regex_match_inner(inner, text, 0, i) {
            return true;
        }
    }
    false
}

fn regex_match_inner(pattern: &str, text: &str, pi: usize, ti: usize) -> bool {
    if pi >= pattern.len() {
        // Pattern exhausted — check if we need full match
        return true;
    }
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();

    // Handle \d (digit), \w (word), \s (whitespace)
    if pb[pi] == b'\\' && pi + 1 < pattern.len() {
        let matches_char = |c: u8| -> bool {
            match pb[pi + 1] {
                b'd' => c.is_ascii_digit(),
                b'w' => c.is_ascii_alphanumeric() || c == b'_',
                b's' => c.is_ascii_whitespace(),
                other => c == other,
            }
        };
        // Check for quantifier {N,M}
        if pi + 2 < pattern.len() && pb[pi + 2] == b'{' {
            if let Some(close) = pattern[pi+2..].find('}') {
                let quant = &pattern[pi+3..pi+2+close];
                let (min, max) = if let Some(comma) = quant.find(',') {
                    let mn: usize = quant[..comma].parse().unwrap_or(0);
                    let mx: usize = quant[comma+1..].parse().unwrap_or(mn);
                    (mn, mx)
                } else {
                    let n: usize = quant.parse().unwrap_or(1);
                    (n, n)
                };
                let next_pi = pi + 2 + close + 1;
                // Try matching min..=max digits
                let mut count = 0;
                let mut t = ti;
                while count < max && t < tb.len() && matches_char(tb[t]) {
                    count += 1;
                    t += 1;
                    if count >= min && regex_match_inner(pattern, text, next_pi, t) {
                        return true;
                    }
                }
                return count >= min && regex_match_inner(pattern, text, next_pi, ti + count);
            }
        }
        // Single char match
        if ti < tb.len() && matches_char(tb[ti]) {
            return regex_match_inner(pattern, text, pi + 2, ti + 1);
        }
        return false;
    }

    // Literal character
    if ti < tb.len() && pb[pi] == tb[ti] {
        return regex_match_inner(pattern, text, pi + 1, ti + 1);
    }

    // . matches any
    if pb[pi] == b'.' && ti < tb.len() {
        return regex_match_inner(pattern, text, pi + 1, ti + 1);
    }

    false
}
