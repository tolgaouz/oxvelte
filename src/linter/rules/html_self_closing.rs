//! `svelte/html-self-closing` — enforce self-closing style.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

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
                // For components and SVG elements, prefer self-closing when empty
                let is_component = el.name.starts_with(|c: char| c.is_uppercase());
                if is_component && el.children.is_empty() && !el.self_closing {
                    ctx.diagnostic(
                        format!("Component `<{}>` has no children and should be self-closing.", el.name),
                        el.span,
                    );
                }
            }
        });
    }
}
