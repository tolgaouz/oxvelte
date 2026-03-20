//! `svelte/no-at-debug-tags` — disallow the use of `{@debug}`.
//! ⭐ Recommended, 💡 Suggestion

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoAtDebugTags;

impl Rule for NoAtDebugTags {
    fn name(&self) -> &'static str {
        "svelte/no-at-debug-tags"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::DebugTag(tag) = node {
                ctx.diagnostic(
                    "Unexpected `{@debug}` tag. Remove before deploying to production.",
                    tag.span,
                );
            }
        });
    }
}
