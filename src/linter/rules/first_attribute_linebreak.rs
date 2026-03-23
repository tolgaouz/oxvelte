//! `svelte/first-attribute-linebreak` — enforce the location of first attribute.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct FirstAttributeLinebreak;

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
        let (multiline_mode, singleline_mode, has_explicit_singleline) = {
            let opts = ctx.config.options.as_ref()
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first());
            let ml = opts.and_then(|v| v.get("multiline"))
                .and_then(|v| v.as_str())
                .unwrap_or("below");
            let sl_opt = opts.and_then(|v| v.get("singleline"))
                .and_then(|v| v.as_str());
            let sl = sl_opt.unwrap_or("beside");
            (ml.to_string(), sl.to_string(), sl_opt.is_some())
        };

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.attributes.is_empty() {
                    return;
                }
                let tag_start = el.span.start as usize;
                let src = ctx.source;
                let tag_src = &src[tag_start..];
                if let Some(name_end) = tag_src.find(|c: char| c.is_whitespace()) {
                    let after_name = &tag_src[name_end..];

                    let is_multi_attr = el.attributes.len() > 1;

                    // Determine the mode based on attribute count
                    // "multiline" config applies when there are multiple attributes
                    // "singleline" config applies when there is a single attribute
                    // By default, only multi-attribute elements are checked (multiline="below")
                    // singleline mode is only enforced when explicitly configured
                    let mode = if is_multi_attr {
                        multiline_mode.as_str()
                    } else {
                        singleline_mode.as_str()
                    };

                    // Skip single-attribute elements when using default singleline config
                    // (the default "beside" for singleline is a no-op unless explicitly set)
                    if !is_multi_attr && !has_explicit_singleline {
                        return;
                    }

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
