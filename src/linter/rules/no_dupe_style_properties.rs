//! `svelte/no-dupe-style-properties` — disallow duplicate style properties.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};
use rustc_hash::FxHashSet;

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
                let mut seen_style_props: FxHashSet<String> = FxHashSet::default();

                for attr in &el.attributes {
                    match attr {
                        // Check style: directives
                        Attribute::Directive {
                            kind: DirectiveKind::StyleDirective,
                            name,
                            span,
                            ..
                        } => {
                            if !seen_style_props.insert(name.clone()) {
                                ctx.diagnostic(
                                    format!("Duplicate property '{}'.", name),
                                    *span,
                                );
                            }
                        }
                        // Check inline style="..." attributes
                        Attribute::NormalAttribute { name, value, span } if name == "style" => {
                            check_style_value(value, &mut seen_style_props, *span, ctx);
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
    seen: &mut FxHashSet<String>,
    span: oxc::span::Span,
    ctx: &mut LintContext<'_>,
) {
    match value {
        AttributeValue::Static(s) => {
            extract_style_props(s, seen, span, ctx);
        }
        AttributeValue::Concat(parts) => {
            for part in parts {
                match part {
                    AttributeValuePart::Static(s) => {
                        extract_style_props(s, seen, span, ctx);
                    }
                    AttributeValuePart::Expression(expr) => {
                        // Extract property names from string literals within the expression.
                        // Collect unique props from this expression and check against seen.
                        let expr_props = extract_props_from_expression(expr);
                        for prop in expr_props {
                            if !seen.insert(prop.clone()) {
                                ctx.diagnostic(
                                    format!("Duplicate property '{}'.", prop),
                                    span,
                                );
                            }
                        }
                    }
                }
            }
        }
        AttributeValue::Expression(expr) => {
            let expr_props = extract_props_from_expression(expr);
            for prop in expr_props {
                if !seen.insert(prop.clone()) {
                    ctx.diagnostic(
                        format!("Duplicate property '{}'.", prop),
                        span,
                    );
                }
            }
        }
        _ => {}
    }
}

fn extract_style_props(
    text: &str,
    seen: &mut FxHashSet<String>,
    span: oxc::span::Span,
    ctx: &mut LintContext<'_>,
) {
    for prop in collect_props_from_css_text(text) {
        if !seen.insert(prop.clone()) {
            ctx.diagnostic(
                format!("Duplicate property '{}'.", prop),
                span,
            );
        }
    }
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
