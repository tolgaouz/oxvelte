//! `svelte/no-unknown-style-directive-property` — disallow unknown CSS properties in style directives.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

const KNOWN_CSS_PROPERTIES: &[&str] = &[
    "accent-color", "align-content", "align-items", "align-self", "all", "animation",
    "animation-delay", "animation-direction", "animation-duration", "animation-fill-mode",
    "animation-iteration-count", "animation-name", "animation-play-state",
    "animation-timing-function", "appearance", "aspect-ratio",
    "backdrop-filter", "backface-visibility", "background", "background-attachment",
    "background-blend-mode", "background-clip", "background-color", "background-image",
    "background-origin", "background-position", "background-repeat", "background-size",
    "block-size", "border", "border-block", "border-block-color", "border-block-end",
    "border-block-end-color", "border-block-end-style", "border-block-end-width",
    "border-block-start", "border-block-start-color", "border-block-start-style",
    "border-block-start-width", "border-block-style", "border-block-width", "border-bottom",
    "border-bottom-color", "border-bottom-left-radius", "border-bottom-right-radius",
    "border-bottom-style", "border-bottom-width", "border-collapse", "border-color",
    "border-image", "border-inline", "border-inline-color", "border-inline-end",
    "border-inline-end-color", "border-inline-end-style", "border-inline-end-width",
    "border-inline-start", "border-inline-start-color", "border-inline-start-style",
    "border-inline-start-width", "border-inline-style", "border-inline-width", "border-left", "border-radius", "border-right",
    "border-spacing", "border-style", "border-top", "border-top-color",
    "border-top-left-radius", "border-top-right-radius", "border-top-style",
    "border-top-width", "border-width", "bottom", "box-decoration-break", "box-shadow",
    "box-sizing", "break-after", "break-before", "break-inside",
    "caption-side", "caret-color", "clear", "clip", "clip-path", "clip-rule",
    "color", "color-interpolation", "color-interpolation-filters", "color-scheme", "column-count",
    "column-fill", "column-gap", "column-rule", "column-span", "column-width", "columns",
    "contain", "contain-intrinsic-size", "container", "container-name", "container-type",
    "content", "content-visibility", "counter-increment", "counter-reset", "counter-set", "cursor",
    "cx", "cy",
    "d", "direction", "display", "dominant-baseline",
    "empty-cells",
    "fill", "fill-opacity", "fill-rule",
    "filter", "flex", "flex-basis", "flex-direction", "flex-flow", "flex-grow",
    "flex-shrink", "flex-wrap", "float", "flood-color", "flood-opacity",
    "font", "font-family", "font-feature-settings",
    "font-kerning", "font-optical-sizing", "font-size", "font-size-adjust", "font-stretch", "font-style",
    "font-variant", "font-variant-caps", "font-variant-east-asian", "font-variant-ligatures",
    "font-variant-numeric", "font-variant-position", "font-variation-settings", "font-weight",
    "gap", "grid", "grid-area", "grid-auto-columns", "grid-auto-flow", "grid-auto-rows",
    "grid-column", "grid-column-end", "grid-column-start", "grid-gap", "grid-row",
    "grid-row-end", "grid-row-start", "grid-template", "grid-template-areas",
    "grid-template-columns", "grid-template-rows",
    "height", "hyphens",
    "image-rendering", "inline-size", "inset", "inset-block", "inset-block-end",
    "inset-block-start", "inset-inline", "inset-inline-end", "inset-inline-start", "isolation",
    "justify-content", "justify-items", "justify-self",
    "left", "letter-spacing", "line-break", "line-height", "list-style",
    "list-style-image", "list-style-position", "list-style-type",
    "margin", "margin-block", "margin-block-end", "margin-block-start",
    "margin-bottom", "margin-inline", "margin-inline-end", "margin-inline-start",
    "margin-left", "margin-right", "margin-top", "max-block-size", "max-height", "max-inline-size",
    "max-width", "min-block-size", "min-height", "min-inline-size", "min-width",
    "mix-blend-mode",
    "marker", "marker-end", "marker-mid", "marker-start",
    "object-fit", "object-position", "offset", "offset-anchor", "offset-distance",
    "offset-path", "offset-position", "offset-rotate",
    "opacity", "order", "orphans", "outline",
    "outline-color", "outline-offset", "outline-style", "outline-width", "overflow",
    "overflow-anchor", "overflow-wrap", "overflow-x", "overflow-y", "overscroll-behavior",
    "padding", "padding-block", "padding-block-end", "padding-block-start",
    "padding-bottom", "padding-inline", "padding-inline-end", "padding-inline-start",
    "padding-left", "padding-right", "padding-top", "page-break-after", "page-break-before",
    "page-break-inside", "paint-order", "perspective", "perspective-origin", "place-content",
    "place-items", "place-self", "pointer-events", "position", "print-color-adjust",
    "quotes",
    "r", "resize", "right", "rotate", "row-gap", "rx", "ry",
    "scale", "scroll-behavior", "scroll-margin", "scroll-padding", "scroll-snap-align",
    "scroll-snap-stop", "scroll-snap-type", "scrollbar-color", "scrollbar-gutter",
    "scrollbar-width",
    "stop-color", "stop-opacity",
    "stroke", "stroke-dasharray", "stroke-dashoffset", "stroke-linecap",
    "stroke-linejoin", "stroke-miterlimit", "stroke-opacity", "stroke-width",
    "tab-size", "table-layout", "text-align", "text-align-last", "text-combine-upright",
    "text-decoration", "text-decoration-color", "text-decoration-line",
    "text-decoration-style", "text-decoration-thickness", "text-indent",
    "text-justify", "text-orientation", "text-overflow", "text-rendering",
    "text-anchor",
    "text-shadow", "text-transform", "text-underline-offset", "text-underline-position",
    "text-wrap", "text-wrap-mode", "text-wrap-style",
    "top", "touch-action", "transform", "transform-origin", "transform-style",
    "transition", "transition-delay", "transition-duration", "transition-property",
    "transition-timing-function", "translate",
    "unicode-bidi", "user-select",
    "vector-effect", "vertical-align", "visibility",
    "white-space", "widows", "width", "will-change", "word-break", "word-spacing",
    "word-wrap", "writing-mode",
    "x", "y",
    "z-index", "zoom",
];

fn is_property_ignored(prop: &str, ignore_list: &[String]) -> bool {
    ignore_list.iter().any(|pat| {
        if let Some(re) = pat.strip_prefix('/').and_then(|s| s.strip_suffix('/')) {
            if let Some(p) = re.strip_prefix('^') {
                p.strip_suffix('$').map_or_else(|| prop.starts_with(p), |exact| prop == exact)
            } else {
                re.strip_suffix('$').map_or_else(|| prop.contains(re), |s| prop.ends_with(s))
            }
        } else {
            prop == pat
        }
    })
}

pub struct NoUnknownStyleDirectiveProperty;

impl Rule for NoUnknownStyleDirectiveProperty {
    fn name(&self) -> &'static str {
        "svelte/no-unknown-style-directive-property"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let ignore_properties: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("ignoreProperties"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::StyleDirective, name, span, .. } = attr {
                        if name.starts_with("--") || ["-moz-", "-webkit-", "-ms-", "-o-"].iter().any(|p| name.starts_with(p)) { continue; }
                        if is_property_ignored(name, &ignore_properties) { continue; }
                        if !KNOWN_CSS_PROPERTIES.contains(&name.as_str()) {
                            ctx.diagnostic(format!("Unexpected unknown style directive property '{}'.", name), *span);
                        }
                    }
                }
            }
        });
    }
}
