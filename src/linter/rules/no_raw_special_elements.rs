//! `svelte/no-raw-special-elements` — checks for invalid raw HTML elements.
//! ⭐ Recommended, 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

/// Elements that must use the `<svelte:*>` form and cannot be used as raw HTML.
const SVELTE_SPECIAL_ELEMENTS: &[&str] = &[
    "svelte:self",
    "svelte:component",
    "svelte:element",
    "svelte:window",
    "svelte:document",
    "svelte:body",
    "svelte:head",
    "svelte:options",
    "svelte:fragment",
    "svelte:boundary",
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
                // Check if this is an invalid capitalized variant like `<Svelte:head>` etc.
                let lower = el.name.to_lowercase();
                if SVELTE_SPECIAL_ELEMENTS.contains(&lower.as_str()) && el.name != lower {
                    ctx.diagnostic(
                        format!(
                            "Invalid special element `<{}>`. Use `<{}>` instead.",
                            el.name, lower
                        ),
                        el.span,
                    );
                }
            }
        });
    }
}
