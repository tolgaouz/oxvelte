//! `svelte/no-raw-special-elements` — checks for raw HTML elements that should use svelte: prefix.
//! ⭐ Recommended, 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

/// Raw HTML element names that should be written as `<svelte:*>` instead.
const RAW_TO_SVELTE: &[(&str, &str)] = &[
    ("head", "svelte:head"),
    ("body", "svelte:body"),
    ("window", "svelte:window"),
    ("document", "svelte:document"),
    ("element", "svelte:element"),
    ("options", "svelte:options"),
];

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
            if let TemplateNode::Element(el) = node {
                for (raw, svelte) in RAW_TO_SVELTE {
                    if el.name == *raw {
                        ctx.diagnostic(
                            format!(
                                "Use `<{}>` instead of raw `<{}>` element.",
                                svelte, raw
                            ),
                            el.span,
                        );
                    }
                }
            }
        });
    }
}
