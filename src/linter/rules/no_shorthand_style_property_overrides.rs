//! `svelte/no-shorthand-style-property-overrides` — disallow shorthand properties that override related longhand properties.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart, DirectiveKind};
use rustc_hash::FxHashSet;

pub struct NoShorthandStylePropertyOverrides;

impl Rule for NoShorthandStylePropertyOverrides {
    fn name(&self) -> &'static str {
        "svelte/no-shorthand-style-property-overrides"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Collect all style properties in order: from style attribute + style: directives
                let mut all_props: Vec<(String, oxc::span::Span)> = Vec::new();

                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name, value, span } if name == "style" => {
                            // Extract props from static text
                            let style_text = collect_style_text(value);
                            for decl in style_text.split(';') {
                                let decl = decl.trim();
                                if let Some(colon) = decl.find(':') {
                                    let prop = decl[..colon].trim().to_lowercase();
                                    if !prop.is_empty() && prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                                        all_props.push((prop, *span));
                                    }
                                }
                            }
                            // Also extract props from expression strings
                            if let AttributeValue::Concat(parts) = value {
                                for part in parts {
                                    if let AttributeValuePart::Expression(expr) = part {
                                        let expr_props = extract_props_from_expression(expr);
                                        for prop in expr_props {
                                            all_props.push((prop, *span));
                                        }
                                    }
                                }
                            }
                        }
                        Attribute::Directive { kind: DirectiveKind::StyleDirective, name, span, .. } => {
                            all_props.push((name.to_lowercase(), *span));
                        }
                        _ => {}
                    }
                }

                // Check for shorthand overrides
                for i in 0..all_props.len() {
                    if let Some(shorthand) = get_shorthand_for(&all_props[i].0) {
                        for j in (i + 1)..all_props.len() {
                            if all_props[j].0 == shorthand {
                                ctx.diagnostic(
                                    format!("Shorthand property '{}' overrides '{}'.", all_props[j].0, all_props[i].0),
                                    all_props[j].1,
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}

fn collect_style_text(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Static(s) => s.clone(),
        AttributeValue::Concat(parts) => {
            parts.iter().map(|p| match p {
                AttributeValuePart::Static(s) => s.as_str(),
                AttributeValuePart::Expression(_) => "",
            }).collect::<Vec<_>>().join("")
        }
        _ => String::new(),
    }
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
                if bytes[i] == b'\\' { i += 2; continue; }
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
                    for decl in literal.split(';') {
                        let decl = decl.trim();
                        if let Some(colon) = decl.find(':') {
                            let prop = decl[..colon].trim().to_lowercase();
                            if !prop.is_empty() && prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                                props.insert(prop);
                            }
                        }
                    }
                    break;
                }
                i += 1;
            }
        }
        i += 1;
    }
    props
}

fn get_shorthand_for(property: &str) -> Option<&'static str> {
    match property {
        "border-top-color" | "border-right-color" | "border-bottom-color" | "border-left-color" => Some("border-color"),
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => Some("border-width"),
        "border-top-style" | "border-right-style" | "border-bottom-style" | "border-left-style" => Some("border-style"),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => Some("padding"),
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => Some("margin"),
        "border-top" | "border-right" | "border-bottom" | "border-left" => Some("border"),
        "background-color" | "background-image" | "background-repeat" | "background-position"
            | "background-size" | "background-attachment" | "background-origin" | "background-clip" => Some("background"),
        "font-style" | "font-variant" | "font-weight" | "font-stretch" | "font-size" | "font-family"
            | "line-height" => Some("font"),
        "flex-grow" | "flex-shrink" | "flex-basis" => Some("flex"),
        "grid-template-rows" | "grid-template-columns" | "grid-template-areas"
            | "grid-auto-rows" | "grid-auto-columns" | "grid-auto-flow" => Some("grid"),
        "overflow-x" | "overflow-y" => Some("overflow"),
        "transition-property" | "transition-duration" | "transition-timing-function" | "transition-delay" => Some("transition"),
        "animation-name" | "animation-duration" | "animation-timing-function" | "animation-delay"
            | "animation-iteration-count" | "animation-direction" | "animation-fill-mode" | "animation-play-state" => Some("animation"),
        "list-style-type" | "list-style-position" | "list-style-image" => Some("list-style"),
        "outline-color" | "outline-style" | "outline-width" => Some("outline"),
        "column-width" | "column-count" => Some("columns"),
        _ => None,
    }
}
