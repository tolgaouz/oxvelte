//! `svelte/no-raw-special-elements` — checks for raw HTML elements that should use svelte: prefix.
//! ⭐ Recommended, 🔧 Fixable

use crate::ast::{Element, TemplateNode};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};

const RAW_NAMES: &[&str] = &["head", "body", "window", "document", "element", "options"];

pub struct NoRawSpecialElements;

impl Rule for NoRawSpecialElements {
    fn name(&self) -> &'static str {
        "svelte/no-raw-special-elements"
    }
    fn is_recommended(&self) -> bool {
        true
    }
    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            if RAW_NAMES.contains(&el.name.as_str()) {
                ctx.diagnostic_with_fix(
                    format!(
                        "Special {} element is deprecated in v5, use svelte:{} instead.",
                        el.name, el.name
                    ),
                    el.span,
                    Fix {
                        span: el.span,
                        replacement: prefixed_special_element_source(ctx.source, el),
                    },
                );
            }
        });
    }
}

fn prefixed_special_element_source(source: &str, el: &Element<'_>) -> String {
    let mut replacement = source[el.span.start as usize..el.span.end as usize].to_string();
    let prefix_len = "svelte:".len();
    let open_insert = (el.name_span.start - el.span.start) as usize;
    replacement.insert_str(open_insert, "svelte:");

    if let Some(end_tag_span) = el.end_tag_span {
        let close_insert = (end_tag_span.start - el.span.start) as usize + 2 + prefix_len;
        if close_insert <= replacement.len() {
            replacement.insert_str(close_insert, "svelte:");
        }
    }

    replacement
}
