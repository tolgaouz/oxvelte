//! `svelte/no-dupe-style-properties` — disallow duplicate style properties.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};
use rustc_hash::{FxHashMap, FxHashSet};

pub struct NoDupeStyleProperties;

impl Rule for NoDupeStyleProperties {
    fn name(&self) -> &'static str {
        "svelte/no-dupe-style-properties"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Track: prop_name -> first occurrence span
                let mut first_seen: FxHashMap<String, oxc::span::Span> = FxHashMap::default();
                let mut reported: FxHashSet<u32> = FxHashSet::default(); // start positions already reported

                for attr in &el.attributes {
                    match attr {
                        // Check style: directives
                        Attribute::Directive {
                            kind: DirectiveKind::StyleDirective,
                            name,
                            span,
                            ..
                        } => {
                            if let Some(first_span) = first_seen.get(name) {
                                // Report first occurrence (if not already)
                                if reported.insert(first_span.start) {
                                    ctx.diagnostic(
                                        format!("Duplicate property '{}'.", name),
                                        *first_span,
                                    );
                                }
                                // Report this occurrence
                                if reported.insert(span.start) {
                                    ctx.diagnostic(
                                        format!("Duplicate property '{}'.", name),
                                        *span,
                                    );
                                }
                            } else {
                                first_seen.insert(name.clone(), *span);
                            }
                        }
                        // Check inline style="..." attributes
                        Attribute::NormalAttribute { name, value, span } if name == "style" => {
                            check_style_value(value, &mut first_seen, &mut reported, *span, ctx);
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

fn check_style_value(
    value: &AttributeValue,
    first_seen: &mut FxHashMap<String, oxc::span::Span>,
    reported: &mut FxHashSet<u32>,
    attr_span: oxc::span::Span,
    ctx: &mut LintContext<'_>,
) {
    let source = ctx.source;
    let attr_text = &source[attr_span.start as usize..attr_span.end as usize];

    match value {
        AttributeValue::Static(s) => {
            check_css_props_in_text(s, first_seen, reported, attr_span, attr_text, ctx);
        }
        AttributeValue::Concat(parts) => {
            for part in parts {
                match part {
                    AttributeValuePart::Static(s) => {
                        check_css_props_in_text(s, first_seen, reported, attr_span, attr_text, ctx);
                    }
                    AttributeValuePart::Expression(expr) => {
                        let expr_props = extract_props_from_expression(expr);
                        for prop in expr_props {
                            if let Some(first_span) = first_seen.get(&prop) {
                                if reported.insert(first_span.start) {
                                    ctx.diagnostic(
                                        format!("Duplicate property '{}'.", prop),
                                        *first_span,
                                    );
                                }
                                // Find position of this property in the source
                                let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, reported)
                                    .unwrap_or(attr_span);
                                if reported.insert(diag_span.start) {
                                    ctx.diagnostic(
                                        format!("Duplicate property '{}'.", prop),
                                        diag_span,
                                    );
                                }
                            } else {
                                // Record first occurrence
                                let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, &FxHashSet::default())
                                    .unwrap_or(attr_span);
                                first_seen.insert(prop, diag_span);
                            }
                        }
                    }
                }
            }
        }
        AttributeValue::Expression(expr) => {
            let expr_props = extract_props_from_expression(expr);
            for prop in expr_props {
                if let Some(first_span) = first_seen.get(&prop) {
                    if reported.insert(first_span.start) {
                        ctx.diagnostic(
                            format!("Duplicate property '{}'.", prop),
                            *first_span,
                        );
                    }
                    let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, reported)
                        .unwrap_or(attr_span);
                    if reported.insert(diag_span.start) {
                        ctx.diagnostic(
                            format!("Duplicate property '{}'.", prop),
                            diag_span,
                        );
                    }
                } else {
                    let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, &FxHashSet::default())
                        .unwrap_or(attr_span);
                    first_seen.insert(prop, diag_span);
                }
            }
        }
        _ => {}
    }
}

fn check_css_props_in_text(
    text: &str,
    first_seen: &mut FxHashMap<String, oxc::span::Span>,
    reported: &mut FxHashSet<u32>,
    attr_span: oxc::span::Span,
    attr_text: &str,
    ctx: &mut LintContext<'_>,
) {
    for prop in collect_props_from_css_text(text) {
        if let Some(first_span) = first_seen.get(&prop) {
            // Report first occurrence
            if reported.insert(first_span.start) {
                ctx.diagnostic(
                    format!("Duplicate property '{}'.", prop),
                    *first_span,
                );
            }
            // Find and report this occurrence
            let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, reported)
                .unwrap_or(attr_span);
            if reported.insert(diag_span.start) {
                ctx.diagnostic(
                    format!("Duplicate property '{}'.", prop),
                    diag_span,
                );
            }
        } else {
            // Record first occurrence with its actual position
            let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, &FxHashSet::default())
                .unwrap_or(attr_span);
            first_seen.insert(prop, diag_span);
        }
    }
}

/// Find the position of a CSS property name in the attribute source text.
/// Returns a span pointing to the property name. Skips positions that have
/// already been reported.
fn find_prop_in_attr(
    attr_text: &str,
    prop: &str,
    attr_start: u32,
    already_reported: &FxHashSet<u32>,
) -> Option<oxc::span::Span> {
    let bytes = attr_text.as_bytes();
    let mut search_start = 0;

    while search_start < attr_text.len() {
        // Search for the property name followed by optional whitespace and ':'
        if let Some(pos) = attr_text[search_start..].find(prop) {
            let abs_pos = search_start + pos;
            let after = abs_pos + prop.len();

            // Check that what follows is whitespace* then ':'
            let rest = &attr_text[after..];
            let trimmed = rest.trim_start();
            if trimmed.starts_with(':') {
                // Check word boundary before the property
                let is_start = abs_pos == 0 || {
                    let prev = bytes[abs_pos - 1];
                    !prev.is_ascii_alphanumeric() && prev != b'-' && prev != b'_'
                };

                if is_start {
                    let span_start = attr_start + abs_pos as u32;
                    let span_end = span_start + prop.len() as u32;

                    // Skip if already reported at this position
                    if !already_reported.contains(&span_start) {
                        return Some(oxc::span::Span::new(span_start, span_end));
                    }
                }
            }

            search_start = abs_pos + 1;
        } else {
            break;
        }
    }
    None
}

fn collect_props_from_css_text(text: &str) -> Vec<String> {
    let mut props = Vec::new();
    for decl in text.split(';') {
        let decl = decl.trim();
        if decl.is_empty() { continue; }
        if let Some(colon_pos) = decl.find(':') {
            let prop = decl[..colon_pos].trim().to_lowercase();
            if !prop.is_empty() && prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                props.push(prop);
            }
        }
    }
    props
}

/// Extract unique CSS property names from string literals within a JS expression.
/// Returns a deduplicated set (so ternary branches with the same prop don't double-count).
fn extract_props_from_expression(expr: &str) -> FxHashSet<String> {
    let mut props = FxHashSet::default();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'\'' || ch == b'"' || ch == b'`' {
            i += 1;
            let start = i;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if ch == b'`' && bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    let mut depth = 1;
                    i += 2;
                    while i < bytes.len() && depth > 0 {
                        if bytes[i] == b'{' { depth += 1; }
                        if bytes[i] == b'}' { depth -= 1; }
                        i += 1;
                    }
                    continue;
                }
                if bytes[i] == ch {
                    let literal = &expr[start..i];
                    props.extend(collect_props_from_css_text(literal));
                    break;
                }
                i += 1;
            }
        }
        i += 1;
    }
    props
}
