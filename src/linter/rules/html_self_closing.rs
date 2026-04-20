//! `svelte/html-self-closing` — enforce self-closing style.
//! 🔧 Fixable
//!
//! Vendor reference: `vendor/eslint-plugin-svelte/.../src/rules/html-self-closing.ts`.
//! Vendor bails on non-empty elements up front (line 195), classifies into six kinds
//! (component/svelte/void/svg/math/normal), reads per-kind options or a preset, then:
//! - "always" + non-self-closing: remove children, insert `/` before `>`, drop `</name>`.
//! - "never" + self-closing: remove `/`, append `</name>` (skipped for void).
//!
//! We express those multi-yield vendor fixes as single contiguous span replacements using
//! `Element::start_tag_end` (byte offset of `>`) and `Element::end_tag_span` recorded by
//! the template parser — no byte scanning at rule time.

use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use crate::ast::{Element, TemplateNode};
use oxc::span::Span;

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
            let TemplateNode::Element(el) = node else { return };
            if !is_element_empty(el) { return; }

            let (opt, label) = classify(el, &component_opt, &normal_opt, &void_opt, &svg_opt, &math_opt, &svelte_opt);
            if opt == "ignore" { return; }
            let should_be_closed = opt == "always";

            if should_be_closed && !el.self_closing {
                report(el, label, true, ctx);
            } else if !should_be_closed && el.self_closing {
                report(el, label, false, ctx);
            }
        });
    }
}

fn is_element_empty(el: &Element) -> bool {
    el.children.is_empty() || el.children.iter().all(|c|
        matches!(c, TemplateNode::Text(t) if t.data.chars().all(|ch| ch.is_whitespace())))
}

fn classify<'o>(
    el: &Element,
    component_opt: &'o str, normal_opt: &'o str, void_opt: &'o str,
    svg_opt: &'o str, math_opt: &'o str, svelte_opt: &'o str,
) -> (&'o str, &'static str) {
    if el.name.starts_with("svelte:") {
        (svelte_opt, "Svelte special elements")
    } else if el.name.starts_with(|c: char| c.is_uppercase()) || el.name.contains('.') {
        (component_opt, "Svelte custom components")
    } else if VOID_ELEMENTS.contains(&el.name.as_str()) {
        (void_opt, "HTML void elements")
    } else if is_svg_element(&el.name) {
        (svg_opt, "SVG elements")
    } else if is_math_element(&el.name) {
        (math_opt, "MathML elements")
    } else {
        (normal_opt, "HTML elements")
    }
}

fn report(el: &Element, label: &str, should_be_closed: bool, ctx: &mut LintContext) {
    // Vendor loc (lines 160–166): start = startTag.range[1] - (selfClosing ? 2 : 1),
    // i.e. the first bracket char — `/` for `/>`, `>` for `>`. `start_tag_end` is the `>` byte.
    let bracket_char = if el.self_closing { el.start_tag_end - 1 } else { el.start_tag_end };
    let diag_span = Span::new(bracket_char, el.span.end);

    if should_be_closed {
        // Collapse `>...</name>` to `/>` (or just insert `/` before `>` for void elements).
        let (fix_span, replacement) = if let Some(end) = el.end_tag_span {
            (Span::new(el.start_tag_end, end.end), "/>".to_string())
        } else {
            (Span::new(el.start_tag_end, el.start_tag_end), "/".to_string())
        };
        ctx.diagnostic_with_fix(
            format!("Require self-closing on {}.", label),
            diag_span,
            Fix { span: fix_span, replacement },
        );
    } else {
        // Replace `/>` with `></name>` (or just `>` for void elements).
        let slash_start = el.start_tag_end - 1;
        let is_void = VOID_ELEMENTS.contains(&el.name.as_str());
        let replacement = if is_void { ">".to_string() } else { format!("></{}>", el.name) };
        ctx.diagnostic_with_fix(
            format!("Disallow self-closing on {}.", label),
            diag_span,
            Fix { span: Span::new(slash_start, el.span.end), replacement },
        );
    }
}
