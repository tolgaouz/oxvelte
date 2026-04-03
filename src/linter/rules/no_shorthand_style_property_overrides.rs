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
            let TemplateNode::Element(el) = node else { return };
            let mut props: Vec<(String, oxc::span::Span)> = Vec::new();
            for attr in &el.attributes {
                match attr {
                    Attribute::NormalAttribute { name, value, span } if name == "style" => collect_style_props(value, *span, &mut props),
                    Attribute::Directive { kind: DirectiveKind::StyleDirective, name, span, .. } => props.push((name.to_lowercase(), *span)),
                    _ => {}
                }
            }
            for i in 0..props.len() {
                for sh in get_shorthands_for(&props[i].0) {
                    for j in (i + 1)..props.len() {
                        if props[j].0 == *sh {
                            ctx.diagnostic(format!("Unexpected shorthand '{}' after '{}'.", props[j].0, props[i].0), props[j].1);
                        }
                    }
                }
            }
        });
    }
}

fn parse_css_prop(decl: &str) -> Option<String> {
    let prop = decl[..decl.find(':')?].trim().to_lowercase();
    if !prop.is_empty() && prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { Some(prop) } else { None }
}

fn collect_static_props(text: &str, span: oxc::span::Span, out: &mut Vec<(String, oxc::span::Span)>) {
    for decl in text.split(';') {
        if let Some(prop) = parse_css_prop(decl.trim()) { out.push((prop, span)); }
    }
}

fn collect_style_props(value: &AttributeValue, span: oxc::span::Span, out: &mut Vec<(String, oxc::span::Span)>) {
    match value {
        AttributeValue::Static(s) => collect_static_props(s, span, out),
        AttributeValue::Concat(parts) => for part in parts {
            match part {
                AttributeValuePart::Static(s) => collect_static_props(s, span, out),
                AttributeValuePart::Expression(e) => for p in extract_props_from_expression(e) { out.push((p, span)); },
            }
        },
        _ => {}
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
                    for decl in expr[start..i].split(';') {
                        if let Some(prop) = parse_css_prop(decl.trim()) { props.insert(prop); }
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
