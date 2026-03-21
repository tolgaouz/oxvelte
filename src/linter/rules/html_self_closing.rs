//! `svelte/html-self-closing` — enforce self-closing style.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

fn is_svg_element(name: &str) -> bool {
    matches!(name, "svg" | "circle" | "ellipse" | "line" | "path" | "polygon"
        | "polyline" | "rect" | "use" | "image" | "text" | "tspan" | "g"
        | "defs" | "symbol" | "clipPath" | "mask" | "pattern" | "marker"
        | "linearGradient" | "radialGradient" | "stop" | "filter" | "feBlend"
        | "feColorMatrix" | "feComponentTransfer" | "feComposite" | "feConvolveMatrix"
        | "feDiffuseLighting" | "feDisplacementMap" | "feFlood" | "feGaussianBlur"
        | "feImage" | "feMerge" | "feMergeNode" | "feMorphology" | "feOffset"
        | "feSpecularLighting" | "feTile" | "feTurbulence" | "animate"
        | "animateMotion" | "animateTransform" | "set" | "foreignObject"
        | "desc" | "title" | "metadata")
}

fn is_math_element(name: &str) -> bool {
    matches!(name, "math" | "mi" | "mn" | "mo" | "ms" | "mspace" | "mtext"
        | "mfrac" | "mroot" | "msqrt" | "mrow" | "msub" | "msup" | "msubsup"
        | "munder" | "mover" | "munderover" | "mtable" | "mtr" | "mtd"
        | "maligngroup" | "malignmark" | "maction" | "menclose" | "merror"
        | "mfenced" | "mglyph" | "mlabeledtr" | "mmultiscripts" | "mpadded"
        | "mphantom" | "mprescripts" | "mstyle" | "none" | "semantics"
        | "annotation" | "annotation-xml")
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input",
    "link", "meta", "param", "source", "track", "wbr",
];

pub struct HtmlSelfClosing;

impl Rule for HtmlSelfClosing {
    fn name(&self) -> &'static str {
        "svelte/html-self-closing"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let is_component = el.name.starts_with(|c: char| c.is_uppercase())
                    || el.name.contains(':') || el.name.contains('.');
                let is_void = VOID_ELEMENTS.contains(&el.name.as_str());
                // SVG/MathML elements can self-close
                let is_svg_or_math = is_svg_element(&el.name) || is_math_element(&el.name);
                let is_html = !is_component && !is_void && !is_svg_or_math;
                let has_only_whitespace = el.children.iter().all(|c| {
                    if let TemplateNode::Text(t) = c { t.data.trim().is_empty() } else { false }
                });

                // HTML elements: disallow self-closing (default)
                if is_html && el.self_closing {
                    ctx.diagnostic(
                        "Disallow self-closing on HTML elements.",
                        el.span,
                    );
                }

                // Components: require self-closing when empty or whitespace-only
                if is_component && !el.self_closing && (el.children.is_empty() || has_only_whitespace) {
                    ctx.diagnostic(
                        "Require self-closing on Svelte custom components.",
                        el.span,
                    );
                }

                // Void elements: require self-closing (default)
                if is_void && !el.self_closing {
                    ctx.diagnostic(
                        "Require self-closing on HTML void elements.",
                        el.span,
                    );
                }
            }
        });
    }
}
