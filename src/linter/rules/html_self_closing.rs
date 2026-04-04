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
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let preset = opts.and_then(|v| v.as_str());

        let (component_opt, normal_opt, void_opt, svg_opt, math_opt, svelte_opt) = if let Some(p) = preset {
            match p {
                "html" => ("never", "never", "always", "always", "never", "always"),
                "none" => ("never", "never", "never", "never", "never", "never"),
                "all" => ("always", "always", "always", "always", "always", "always"),
                _ => ("always", "never", "always", "always", "always", "always"),
            }
        } else {
            (
                opts.and_then(|o| o.get("component")).and_then(|v| v.as_str()).unwrap_or("always"),
                opts.and_then(|o| o.get("normal")).and_then(|v| v.as_str()).unwrap_or("never"),
                opts.and_then(|o| o.get("void")).and_then(|v| v.as_str()).unwrap_or("always"),
                opts.and_then(|o| o.get("svg")).and_then(|v| v.as_str()).unwrap_or("always"),
                opts.and_then(|o| o.get("math")).and_then(|v| v.as_str()).unwrap_or("never"),
                opts.and_then(|o| o.get("svelte")).and_then(|v| v.as_str()).unwrap_or("always"),
            )
        };

        let component_opt = component_opt.to_string();
        let normal_opt = normal_opt.to_string();
        let void_opt = void_opt.to_string();
        let svg_opt = svg_opt.to_string();
        let math_opt = math_opt.to_string();
        let svelte_opt = svelte_opt.to_string();

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let is_void = VOID_ELEMENTS.contains(&el.name.as_str());
                let (opt, label) = if el.name.starts_with("svelte:") {
                    (&svelte_opt, "Svelte special elements")
                } else if el.name.starts_with(|c: char| c.is_uppercase()) || el.name.contains('.') {
                    (&component_opt, "Svelte custom components")
                } else if is_void {
                    (&void_opt, "HTML void elements")
                } else if is_svg_element(&el.name) {
                    (&svg_opt, "SVG elements")
                } else if is_math_element(&el.name) {
                    (&math_opt, "MathML elements")
                } else {
                    (&normal_opt, "HTML elements")
                };
                if opt == "ignore" { return; }
                let is_empty = el.children.is_empty() || el.children.iter().all(|c|
                    matches!(c, TemplateNode::Text(t) if t.data.trim().is_empty()));
                if opt == "never" && el.self_closing {
                    ctx.diagnostic(format!("Disallow self-closing on {}.", label), el.span);
                } else if opt == "always" && !el.self_closing && (is_void || is_empty) {
                    ctx.diagnostic(format!("Require self-closing on {}.", label), el.span);
                }
            }
        });
    }
}
