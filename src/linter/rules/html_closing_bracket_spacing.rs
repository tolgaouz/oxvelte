//! `svelte/html-closing-bracket-spacing` — enforce consistent spacing before
//! the closing bracket of self-closing HTML elements.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct HtmlClosingBracketSpacing;

impl Rule for HtmlClosingBracketSpacing {
    fn name(&self) -> &'static str {
        "svelte/html-closing-bracket-spacing"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.self_closing {
                    let end = el.span.end as usize;
                    // The closing should be `/>`. Check for space before it: ` />` is expected.
                    if end >= 2 {
                        let before_close = &ctx.source[..end];
                        if before_close.ends_with("/>") {
                            // Check character before `/>`
                            let pre = &before_close[..before_close.len() - 2];
                            if !pre.ends_with(' ') && !pre.ends_with('\n') {
                                let pos = (end - 2) as u32;
                                ctx.diagnostic(
                                    "Expected a space before `/>` in self-closing element.",
                                    Span::new(pos, pos + 2),
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}
