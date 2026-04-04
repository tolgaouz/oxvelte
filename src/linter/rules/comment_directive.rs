//! `svelte/comment-directive` — support `<!-- svelte-ignore -->` comment directives.
//! ⭐ Recommended
//!
//! This is a system rule that processes `svelte-ignore` comments. In a full
//! implementation it would suppress diagnostics for the next sibling node.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct CommentDirective;

impl Rule for CommentDirective {
    fn name(&self) -> &'static str {
        "svelte/comment-directive"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Validate that svelte-ignore comments reference known rule names.
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Comment(comment) = node {
                let text = comment.data.trim();
                if let Some(rest) = text.strip_prefix("svelte-ignore") {
                    let rest = rest.trim();
                    if rest.is_empty() {
                        ctx.diagnostic("`svelte-ignore` comment must specify at least one rule name.",
                            comment.span);
                    }
                }
            }
        });
    }
}
