//! `svelte/no-shorthand-style-property-overrides` — disallow shorthand properties that override related longhand properties.
//! ⭐ Recommended

use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};
use crate::linter::{walk_template_nodes, LintContext, Rule};
use oxc::span::Span;
use rustc_hash::FxHashSet;

type StyleDecl = (String, Span);
type StyleDeclSet = Vec<StyleDecl>;

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
            let TemplateNode::Element(el) = node else {
                return;
            };
            let mut decl_sets: Vec<StyleDeclSet> = Vec::new();
            for attr in &el.attributes {
                match attr {
                    Attribute::NormalAttribute { name, value, span } if name == "style" => {
                        collect_style_decl_sets(value, *span, &mut decl_sets)
                    }
                    Attribute::Directive {
                        kind: DirectiveKind::StyleDirective,
                        name,
                        span,
                        ..
                    } => decl_sets.push(vec![(name.to_lowercase(), *span)]),
                    _ => {}
                }
            }
            let mut before_declarations = FxHashSet::default();
            for decls in decl_sets {
                for (prop, span) in &decls {
                    let (prefix, normalized) = split_vendor_prefix(prop);
                    let Some(longhands) = longhands_for_shorthand(normalized) else {
                        continue;
                    };
                    for longhand in longhands {
                        let original = format!("{prefix}{longhand}");
                        if before_declarations.contains(&original) {
                            ctx.diagnostic(
                                format!("Unexpected shorthand '{}' after '{}'.", prop, original),
                                *span,
                            );
                        }
                    }
                }
                for (prop, _) in decls {
                    before_declarations.insert(prop);
                }
            }
        });
    }
}

fn parse_css_prop(decl: &str) -> Option<String> {
    let prop = decl[..decl.find(':')?].trim().to_lowercase();
    if !prop.is_empty() && prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Some(prop)
    } else {
        None
    }
}

fn collect_static_props(text: &str, span: Span, out: &mut Vec<StyleDeclSet>) {
    for decl in text.split(';') {
        if let Some(prop) = parse_css_prop(decl.trim()) {
            out.push(vec![(prop, span)]);
        }
    }
}

fn collect_style_decl_sets(value: &AttributeValue, span: Span, out: &mut Vec<StyleDeclSet>) {
    match value {
        AttributeValue::Static(s) => collect_static_props(s, span, out),
        AttributeValue::Concat(parts) => {
            for part in parts {
                match part {
                    AttributeValuePart::Static(s) => collect_static_props(s, span, out),
                    AttributeValuePart::Expression(e) => {
                        let props = extract_props_from_expression(e);
                        if !props.is_empty() {
                            out.push(props.into_iter().map(|p| (p, span)).collect());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn extract_props_from_expression(expr: &str) -> Vec<String> {
    let mut seen = FxHashSet::default();
    let mut props = Vec::new();
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
                        if bytes[i] == b'{' {
                            depth += 1;
                        }
                        if bytes[i] == b'}' {
                            depth -= 1;
                        }
                        i += 1;
                    }
                    continue;
                }
                if bytes[i] == ch {
                    for decl in expr[start..i].split(';') {
                        if let Some(prop) = parse_css_prop(decl.trim()) {
                            if seen.insert(prop.clone()) {
                                props.push(prop);
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

fn split_vendor_prefix(prop: &str) -> (&str, &str) {
    let bytes = prop.as_bytes();
    if bytes.first() != Some(&b'-') {
        return ("", prop);
    }
    let mut end = 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    if end > 1 && bytes.get(end) == Some(&b'-') {
        (&prop[..=end], &prop[end + 1..])
    } else {
        ("", prop)
    }
}

fn longhands_for_shorthand(property: &str) -> Option<&'static [&'static str]> {
    Some(match property {
        "margin" => &["margin-top", "margin-bottom", "margin-left", "margin-right"],
        "padding" => &[
            "padding-top",
            "padding-bottom",
            "padding-left",
            "padding-right",
        ],
        "background" => &[
            "background-image",
            "background-size",
            "background-position",
            "background-repeat",
            "background-origin",
            "background-clip",
            "background-attachment",
            "background-color",
        ],
        "font" => &[
            "font-style",
            "font-variant",
            "font-weight",
            "font-stretch",
            "font-size",
            "font-family",
            "line-height",
        ],
        "border" => &[
            "border-top-width",
            "border-bottom-width",
            "border-left-width",
            "border-right-width",
            "border-top-style",
            "border-bottom-style",
            "border-left-style",
            "border-right-style",
            "border-top-color",
            "border-bottom-color",
            "border-left-color",
            "border-right-color",
        ],
        "border-top" => &["border-top-width", "border-top-style", "border-top-color"],
        "border-bottom" => &[
            "border-bottom-width",
            "border-bottom-style",
            "border-bottom-color",
        ],
        "border-left" => &[
            "border-left-width",
            "border-left-style",
            "border-left-color",
        ],
        "border-right" => &[
            "border-right-width",
            "border-right-style",
            "border-right-color",
        ],
        "border-width" => &[
            "border-top-width",
            "border-bottom-width",
            "border-left-width",
            "border-right-width",
        ],
        "border-style" => &[
            "border-top-style",
            "border-bottom-style",
            "border-left-style",
            "border-right-style",
        ],
        "border-color" => &[
            "border-top-color",
            "border-bottom-color",
            "border-left-color",
            "border-right-color",
        ],
        "list-style" => &["list-style-type", "list-style-position", "list-style-image"],
        "border-radius" => &[
            "border-top-right-radius",
            "border-top-left-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ],
        "transition" => &[
            "transition-delay",
            "transition-duration",
            "transition-property",
            "transition-timing-function",
        ],
        "animation" => &[
            "animation-name",
            "animation-duration",
            "animation-timing-function",
            "animation-delay",
            "animation-iteration-count",
            "animation-direction",
            "animation-fill-mode",
            "animation-play-state",
        ],
        "border-block-end" => &[
            "border-block-end-width",
            "border-block-end-style",
            "border-block-end-color",
        ],
        "border-block-start" => &[
            "border-block-start-width",
            "border-block-start-style",
            "border-block-start-color",
        ],
        "border-image" => &[
            "border-image-source",
            "border-image-slice",
            "border-image-width",
            "border-image-outset",
            "border-image-repeat",
        ],
        "border-inline-end" => &[
            "border-inline-end-width",
            "border-inline-end-style",
            "border-inline-end-color",
        ],
        "border-inline-start" => &[
            "border-inline-start-width",
            "border-inline-start-style",
            "border-inline-start-color",
        ],
        "column-rule" => &[
            "column-rule-width",
            "column-rule-style",
            "column-rule-color",
        ],
        "columns" => &["column-width", "column-count"],
        "flex" => &["flex-grow", "flex-shrink", "flex-basis"],
        "flex-flow" => &["flex-direction", "flex-wrap"],
        "grid" => &[
            "grid-template-rows",
            "grid-template-columns",
            "grid-template-areas",
            "grid-auto-rows",
            "grid-auto-columns",
            "grid-auto-flow",
            "grid-column-gap",
            "grid-row-gap",
        ],
        "grid-area" => &[
            "grid-row-start",
            "grid-column-start",
            "grid-row-end",
            "grid-column-end",
        ],
        "grid-column" => &["grid-column-start", "grid-column-end"],
        "grid-gap" => &["grid-row-gap", "grid-column-gap"],
        "grid-row" => &["grid-row-start", "grid-row-end"],
        "grid-template" => &[
            "grid-template-columns",
            "grid-template-rows",
            "grid-template-areas",
        ],
        "outline" => &["outline-color", "outline-style", "outline-width"],
        "text-decoration" => &[
            "text-decoration-color",
            "text-decoration-style",
            "text-decoration-line",
        ],
        "text-emphasis" => &["text-emphasis-style", "text-emphasis-color"],
        "mask" => &[
            "mask-image",
            "mask-mode",
            "mask-position",
            "mask-size",
            "mask-repeat",
            "mask-origin",
            "mask-clip",
            "mask-composite",
        ],
        _ => return None,
    })
}
