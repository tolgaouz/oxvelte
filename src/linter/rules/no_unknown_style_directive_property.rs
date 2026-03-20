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
    "border-block-start", "border-block-style", "border-block-width", "border-bottom",
    "border-bottom-color", "border-bottom-left-radius", "border-bottom-right-radius",
    "border-bottom-style", "border-bottom-width", "border-collapse", "border-color",
    "border-image", "border-inline", "border-left", "border-radius", "border-right",
    "border-spacing", "border-style", "border-top", "border-top-color",
    "border-top-left-radius", "border-top-right-radius", "border-top-style",
    "border-top-width", "border-width", "bottom", "box-decoration-break", "box-shadow",
    "box-sizing", "break-after", "break-before", "break-inside",
    "caption-side", "caret-color", "clear", "clip", "clip-path", "color", "column-count",
    "column-fill", "column-gap", "column-rule", "column-span", "column-width", "columns",
    "contain", "content", "counter-increment", "counter-reset", "cursor",
    "direction", "display",
    "empty-cells",
    "filter", "flex", "flex-basis", "flex-direction", "flex-flow", "flex-grow",
    "flex-shrink", "flex-wrap", "float", "font", "font-family", "font-feature-settings",
    "font-kerning", "font-size", "font-size-adjust", "font-stretch", "font-style",
    "font-variant", "font-weight",
    "gap", "grid", "grid-area", "grid-auto-columns", "grid-auto-flow", "grid-auto-rows",
    "grid-column", "grid-column-end", "grid-column-start", "grid-gap", "grid-row",
    "grid-row-end", "grid-row-start", "grid-template", "grid-template-areas",
    "grid-template-columns", "grid-template-rows",
    "height", "hyphens",
    "image-rendering", "inline-size", "inset", "isolation",
    "justify-content", "justify-items", "justify-self",
    "left", "letter-spacing", "line-break", "line-height", "list-style",
    "list-style-image", "list-style-position", "list-style-type",
    "margin", "margin-block", "margin-bottom", "margin-inline", "margin-left",
    "margin-right", "margin-top", "max-block-size", "max-height", "max-inline-size",
    "max-width", "min-block-size", "min-height", "min-inline-size", "min-width",
    "mix-blend-mode",
    "object-fit", "object-position", "opacity", "order", "orphans", "outline",
    "outline-color", "outline-offset", "outline-style", "outline-width", "overflow",
    "overflow-anchor", "overflow-wrap", "overflow-x", "overflow-y", "overscroll-behavior",
    "padding", "padding-block", "padding-bottom", "padding-inline", "padding-left",
    "padding-right", "padding-top", "page-break-after", "page-break-before",
    "page-break-inside", "perspective", "perspective-origin", "place-content",
    "place-items", "place-self", "pointer-events", "position",
    "quotes",
    "resize", "right", "rotate", "row-gap",
    "scale", "scroll-behavior", "scroll-margin", "scroll-padding", "scroll-snap-align",
    "scroll-snap-stop", "scroll-snap-type",
    "tab-size", "table-layout", "text-align", "text-align-last", "text-combine-upright",
    "text-decoration", "text-decoration-color", "text-decoration-line",
    "text-decoration-style", "text-decoration-thickness", "text-indent",
    "text-justify", "text-orientation", "text-overflow", "text-rendering",
    "text-shadow", "text-transform", "text-underline-offset", "text-underline-position",
    "top", "touch-action", "transform", "transform-origin", "transform-style",
    "transition", "transition-delay", "transition-duration", "transition-property",
    "transition-timing-function", "translate",
    "unicode-bidi", "user-select",
    "vertical-align", "visibility",
    "white-space", "widows", "width", "will-change", "word-break", "word-spacing",
    "word-wrap", "writing-mode",
    "z-index", "zoom",
];

pub struct NoUnknownStyleDirectiveProperty;

impl Rule for NoUnknownStyleDirectiveProperty {
    fn name(&self) -> &'static str {
        "svelte/no-unknown-style-directive-property"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::StyleDirective, name, span, .. } = attr {
                        // Allow CSS custom properties (--var)
                        if name.starts_with("--") {
                            continue;
                        }
                        if !KNOWN_CSS_PROPERTIES.contains(&name.as_str()) {
                            ctx.diagnostic(
                                format!("Unknown CSS property '{}'.", name),
                                *span,
                            );
                        }
                    }
                }
            }
        });
    }
}
