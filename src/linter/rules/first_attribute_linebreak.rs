//! `svelte/first-attribute-linebreak` — enforce the location of first attribute.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute};

pub struct FirstAttributeLinebreak;

/// Return the byte offset (start, end) of the given attribute's span.
fn attr_span_range(attr: &Attribute) -> (u32, u32) {
    match attr {
        Attribute::NormalAttribute { span, .. } => (span.start, span.end),
        Attribute::Spread { span } => (span.start, span.end),
        Attribute::Directive { span, .. } => (span.start, span.end),
    }
}

/// Count the line number (0-based) of a byte offset in the source.
fn line_of(src: &str, offset: usize) -> usize {
    src[..offset].chars().filter(|&c| c == '\n').count()
}

impl Rule for FirstAttributeLinebreak {
    fn name(&self) -> &'static str {
        "svelte/first-attribute-linebreak"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Config: { "multiline": "below"|"beside", "singleline": "below"|"beside" }
        // Default: multiline="below", singleline="beside"
        let (multiline_mode, singleline_mode) = {
            let opts = ctx.config.options.as_ref()
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first());
            let ml = opts.and_then(|v| v.get("multiline"))
                .and_then(|v| v.as_str())
                .unwrap_or("below");
            let sl = opts.and_then(|v| v.get("singleline"))
                .and_then(|v| v.as_str())
                .unwrap_or("beside");
            (ml.to_string(), sl.to_string())
        };

        let src = ctx.source;

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.attributes.is_empty() {
                    return;
                }

                let first_attr = el.attributes.first().unwrap();
                let last_attr = el.attributes.last().unwrap();

                let (first_start, _) = attr_span_range(first_attr);
                let (_, last_end) = attr_span_range(last_attr);

                // Classify: if first attr starts on same line as last attr ends → singleline
                // otherwise → multiline
                let first_line = line_of(src, first_start as usize);
                let last_line = line_of(src, last_end as usize);
                let is_singleline = first_line == last_line;

                let mode = if is_singleline {
                    singleline_mode.as_str()
                } else {
                    multiline_mode.as_str()
                };

                let tag_start = el.span.start as usize;
                let tag_src = &src[tag_start..];
                if let Some(name_end) = tag_src.find(|c: char| c.is_whitespace()) {
                    let after_name = &tag_src[name_end..];
                    let first_attr_on_new_line = after_name.starts_with('\n') || after_name.starts_with("\r\n");

                    if mode == "below" && !first_attr_on_new_line {
                        let first_attr_char = after_name.trim_start();
                        if !first_attr_char.is_empty() {
                            ctx.diagnostic(
                                "Expected a linebreak before this attribute.",
                                el.span,
                            );
                        }
                    } else if mode == "beside" && first_attr_on_new_line {
                        ctx.diagnostic(
                            "Expected no linebreak before this attribute.",
                            el.span,
                        );
                    }
                }
            }
        });
    }
}
