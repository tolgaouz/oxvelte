//! `svelte/valid-each-key` — enforce that each blocks with key use a unique identifier.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct ValidEachKey;

impl Rule for ValidEachKey {
    fn name(&self) -> &'static str {
        "svelte/valid-each-key"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::EachBlock(block) = node {
                if let Some(key) = &block.key {
                    // Key should reference a property of the iteration variable
                    // Warn if key is just the iteration variable itself
                    if key.trim() == block.context.trim() {
                        ctx.diagnostic(
                            format!(
                                "Using the iteration variable '{}' directly as the key is not recommended. Use a unique identifier like '{}.id'.",
                                block.context, block.context
                            ),
                            block.span,
                        );
                    }
                }
            }
        });
    }
}
