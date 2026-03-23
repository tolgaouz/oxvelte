//! `svelte/require-each-key` — require keyed `{#each}` block.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct RequireEachKey;

impl Rule for RequireEachKey {
    fn name(&self) -> &'static str {
        "svelte/require-each-key"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::EachBlock(block) = node {
                if block.key.is_none() {
                    // Skip if no context variable (Svelte 5 comma syntax with only index).
                    // Also skip Svelte 5 object literal expressions like { length: N }
                    // but only when there's no `as` context (comma syntax only).
                    if block.context.trim().is_empty() {
                        return;
                    }
                    ctx.diagnostic(
                        "Require a key expression in `{#each}` block for efficient list updates.",
                        block.span,
                    );
                }
            }
        });
    }
}
