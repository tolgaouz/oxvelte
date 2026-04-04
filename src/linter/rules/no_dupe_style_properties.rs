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
                let mut first_seen: FxHashMap<String, oxc::span::Span> = FxHashMap::default();
                let mut reported: FxHashSet<u32> = FxHashSet::default(); // start positions already reported

                for attr in &el.attributes {
                    match attr {
                        Attribute::Directive {
                            kind: DirectiveKind::StyleDirective,
                            name,
                            span,
                            ..
                        } => {
                            if let Some(first_span) = first_seen.get(name) {
                                if reported.insert(first_span.start) {
                                    ctx.diagnostic(format!("Duplicate property '{}'.", name),
                                        *first_span);
                                }
                                if reported.insert(span.start) {
                                    ctx.diagnostic(format!("Duplicate property '{}'.", name),
                                        *span);
                                }
                            } else {
                                first_seen.insert(name.clone(), *span);
                            }
                        }
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
    let attr_text = &ctx.source[attr_span.start as usize..attr_span.end as usize];
    let mut all_props: Vec<String> = Vec::new();
    match value {
        AttributeValue::Static(s) => all_props.extend(collect_props_from_css_text(s)),
        AttributeValue::Concat(parts) => for part in parts {
            match part {
                AttributeValuePart::Static(s) => all_props.extend(collect_props_from_css_text(s)),
                AttributeValuePart::Expression(e) => all_props.extend(extract_props_from_expression(e)),
            }
        },
        AttributeValue::Expression(e) => all_props.extend(extract_props_from_expression(e)),
        _ => {}
    }
    for prop in all_props {
        report_or_record(prop, first_seen, reported, attr_text, attr_span, ctx);
    }
}

fn report_or_record(
    prop: String,
    first_seen: &mut FxHashMap<String, oxc::span::Span>,
    reported: &mut FxHashSet<u32>,
    attr_text: &str,
    attr_span: oxc::span::Span,
    ctx: &mut LintContext<'_>,
) {
    if let Some(first_span) = first_seen.get(&prop) {
        if reported.insert(first_span.start) {
            ctx.diagnostic(format!("Duplicate property '{}'.", prop), *first_span);
        }
        let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, reported)
            .unwrap_or(attr_span);
        if reported.insert(diag_span.start) {
            ctx.diagnostic(format!("Duplicate property '{}'.", prop), diag_span);
        }
    } else {
        let diag_span = find_prop_in_attr(attr_text, &prop, attr_span.start, &FxHashSet::default())
            .unwrap_or(attr_span);
        first_seen.insert(prop, diag_span);
    }
}

fn find_prop_in_attr(
    attr_text: &str,
    prop: &str,
    attr_start: u32,
    already_reported: &FxHashSet<u32>,
) -> Option<oxc::span::Span> {
    let bytes = attr_text.as_bytes();
    let mut search_start = 0;

    while let Some(pos) = attr_text[search_start..].find(prop) {
        let abs = search_start + pos;
        if attr_text[abs + prop.len()..].trim_start().starts_with(':')
            && (abs == 0 || !matches!(bytes[abs - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_'))
        {
            let ss = attr_start + abs as u32;
            if !already_reported.contains(&ss) {
                return Some(oxc::span::Span::new(ss, ss + prop.len() as u32));
            }
        }
        search_start = abs + 1;
    }
    None
}

fn collect_props_from_css_text(text: &str) -> Vec<String> {
    text.split(';').filter_map(|decl| {
        let prop = decl.trim().split_once(':')?.0.trim().to_lowercase();
        (!prop.is_empty() && prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then_some(prop)
    }).collect()
}

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
