//! `svelte/no-at-html-tags` — disallow use of `{@html}` to prevent XSS attack.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoAtHtmlTags;

impl Rule for NoAtHtmlTags {
    fn name(&self) -> &'static str {
        "svelte/no-at-html-tags"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::RawMustacheTag(tag) = node {
                ctx.diagnostic(
                    "`{@html}` can lead to XSS attack.",
                    tag.span,
                );
            }
        });
    }
}
