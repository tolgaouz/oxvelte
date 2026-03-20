//! `svelte/no-restricted-html-elements` — disallow specific HTML elements.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

/// Default set of restricted HTML elements.
const RESTRICTED_ELEMENTS: &[&str] = &[
    "marquee", "blink", "font", "center", "big", "strike",
];

pub struct NoRestrictedHtmlElements;

impl Rule for NoRestrictedHtmlElements {
    fn name(&self) -> &'static str {
        "svelte/no-restricted-html-elements"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let lower = el.name.to_ascii_lowercase();
                if RESTRICTED_ELEMENTS.contains(&lower.as_str()) {
                    ctx.diagnostic(
                        format!("The `<{}>` element is restricted and should not be used.", el.name),
                        el.span,
                    );
                }
            }
        });
    }
}
