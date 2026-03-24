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
                    let shorthands = get_shorthands_for(&all_props[i].0);
                    for shorthand in shorthands {
                        for j in (i + 1)..all_props.len() {
                            if all_props[j].0 == *shorthand {
                                ctx.diagnostic(
                                    format!("Unexpected shorthand '{}' after '{}'.", all_props[j].0, all_props[i].0),
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

/// Returns all shorthands that the given longhand property is a sub-property of.
/// A longhand can belong to more than one shorthand (e.g. `grid-column-start` belongs
/// to both `grid-area` and `grid-column`).
fn get_shorthands_for(property: &str) -> &'static [&'static str] {
    match property {
        // margin
        "margin-top" | "margin-bottom" | "margin-left" | "margin-right" => &["margin"],
        // padding
        "padding-top" | "padding-bottom" | "padding-left" | "padding-right" => &["padding"],
        // background
        "background-image" | "background-size" | "background-position" | "background-repeat"
            | "background-origin" | "background-clip" | "background-attachment"
            | "background-color" => &["background"],
        // font
        "font-style" | "font-variant" | "font-weight" | "font-stretch" | "font-size"
            | "font-family" | "line-height" => &["font"],
        // border (all longhands of the top-level `border` shorthand)
        // border-top, border-bottom, border-left, border-right are themselves shorthands of border
        "border-top" | "border-bottom" | "border-left" | "border-right" => &["border"],
        // border-width longhands → border-width AND border
        "border-top-width" | "border-bottom-width" | "border-left-width" | "border-right-width" => &["border-width", "border"],
        // border-style longhands → border-style AND border
        "border-top-style" | "border-bottom-style" | "border-left-style" | "border-right-style" => &["border-style", "border"],
        // border-color longhands → border-color AND border
        "border-top-color" | "border-bottom-color" | "border-left-color" | "border-right-color" => &["border-color", "border"],
        // list-style
        "list-style-type" | "list-style-position" | "list-style-image" => &["list-style"],
        // border-radius
        "border-top-right-radius" | "border-top-left-radius"
            | "border-bottom-right-radius" | "border-bottom-left-radius" => &["border-radius"],
        // transition
        "transition-delay" | "transition-duration" | "transition-property"
            | "transition-timing-function" => &["transition"],
        // animation
        "animation-name" | "animation-duration" | "animation-timing-function"
            | "animation-delay" | "animation-iteration-count" | "animation-direction"
            | "animation-fill-mode" | "animation-play-state" => &["animation"],
        // border-block-end
        "border-block-end-width" | "border-block-end-style" | "border-block-end-color" => &["border-block-end"],
        // border-block-start
        "border-block-start-width" | "border-block-start-style" | "border-block-start-color" => &["border-block-start"],
        // border-image
        "border-image-source" | "border-image-slice" | "border-image-width"
            | "border-image-outset" | "border-image-repeat" => &["border-image"],
        // border-inline-end
        "border-inline-end-width" | "border-inline-end-style" | "border-inline-end-color" => &["border-inline-end"],
        // border-inline-start
        "border-inline-start-width" | "border-inline-start-style" | "border-inline-start-color" => &["border-inline-start"],
        // column-rule
        "column-rule-width" | "column-rule-style" | "column-rule-color" => &["column-rule"],
        // columns
        "column-width" | "column-count" => &["columns"],
        // flex
        "flex-grow" | "flex-shrink" | "flex-basis" => &["flex"],
        // flex-flow
        "flex-direction" | "flex-wrap" => &["flex-flow"],
        // grid — longhands exclusive to `grid`
        "grid-auto-rows" | "grid-auto-columns" | "grid-auto-flow"
            | "grid-column-gap" | "grid-row-gap" => &["grid"],
        // grid-template-* → grid-template AND grid
        "grid-template-columns" | "grid-template-rows" | "grid-template-areas" => &["grid-template", "grid"],
        // grid-area → grid-area; grid-column-start / grid-column-end also belong to grid-column
        "grid-row-start" | "grid-row-end" => &["grid-row", "grid-area"],
        "grid-column-start" | "grid-column-end" => &["grid-column", "grid-area"],
        // grid-gap
        "grid-gap" => &["grid"],
        // outline
        "outline-color" | "outline-style" | "outline-width" => &["outline"],
        // overflow
        "overflow-x" | "overflow-y" => &["overflow"],
        // text-decoration
        "text-decoration-color" | "text-decoration-style" | "text-decoration-line" => &["text-decoration"],
        // text-emphasis
        "text-emphasis-style" | "text-emphasis-color" => &["text-emphasis"],
        // mask
        "mask-image" | "mask-mode" | "mask-position" | "mask-size" | "mask-repeat"
            | "mask-origin" | "mask-clip" | "mask-composite" => &["mask"],
        _ => &[],
    }
}
