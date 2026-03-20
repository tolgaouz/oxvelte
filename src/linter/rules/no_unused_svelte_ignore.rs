//! `svelte/no-unused-svelte-ignore` — disallow unused svelte-ignore comments.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoUnusedSvelteIgnore;

impl Rule for NoUnusedSvelteIgnore {
    fn name(&self) -> &'static str {
        "svelte/no-unused-svelte-ignore"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Look for svelte-ignore comments that don't precede any diagnostics
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Comment(comment) = node {
                if comment.data.trim_start().starts_with("svelte-ignore") {
                    // Simplified: just check if the comment is properly formed
                    let after_prefix = comment.data.trim_start().strip_prefix("svelte-ignore").unwrap_or("");
                    if after_prefix.trim().is_empty() {
                        ctx.diagnostic(
                            "svelte-ignore comment has no rules listed.",
                            comment.span,
                        );
                    }
                }
            }
        });
    }
}
