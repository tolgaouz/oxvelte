//! `svelte/no-raw-special-elements` — checks for raw HTML elements that should use svelte: prefix.
//! ⭐ Recommended, 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

const RAW_NAMES: &[&str] = &["head", "body", "window", "document", "element", "options"];

pub struct NoRawSpecialElements;

impl Rule for NoRawSpecialElements {
    fn name(&self) -> &'static str { "svelte/no-raw-special-elements" }
    fn is_recommended(&self) -> bool { true }
    fn is_fixable(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            if RAW_NAMES.contains(&el.name.as_str()) {
                ctx.diagnostic(format!("Special {} element is deprecated in v5, use svelte:{} instead.", el.name, el.name), el.span);
            }
        });
    }
}
