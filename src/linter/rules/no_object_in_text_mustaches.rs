//! `svelte/no-object-in-text-mustaches` — disallow objects in text mustache interpolation.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoObjectInTextMustaches;

impl Rule for NoObjectInTextMustaches {
    fn name(&self) -> &'static str {
        "svelte/no-object-in-text-mustaches"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::MustacheTag(tag) = node {
                let expr = tag.expression.trim();
                // Simple heuristic: expression starts with `{` or `[` (object/array literal)
                if expr.starts_with('{') || expr.starts_with('[') {
                    ctx.diagnostic(
                        "Unexpected object/array in text mustache. Objects render as `[object Object]`.",
                        tag.span,
                    );
                }
            }
        });
    }
}
