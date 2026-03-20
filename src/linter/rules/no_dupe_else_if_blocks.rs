//! `svelte/no-dupe-else-if-blocks` — disallow duplicate conditions in `{#if}` / `{:else if}` chains.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoDupeElseIfBlocks;

impl Rule for NoDupeElseIfBlocks {
    fn name(&self) -> &'static str {
        "svelte/no-dupe-else-if-blocks"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::IfBlock(block) = node {
                let mut seen_conditions = vec![block.test.trim().to_string()];
                check_alternate(&block.alternate, &mut seen_conditions, ctx);
            }
        });
    }
}

fn check_alternate(
    alternate: &Option<Box<crate::ast::TemplateNode>>,
    seen: &mut Vec<String>,
    ctx: &mut LintContext<'_>,
) {
    if let Some(alt) = alternate {
        if let TemplateNode::IfBlock(block) = alt.as_ref() {
            let condition = block.test.trim().to_string();
            if !condition.is_empty() {
                if seen.contains(&condition) {
                    ctx.diagnostic(
                        format!(
                            "Duplicate condition `{}` in `{{:else if}}` chain.",
                            condition
                        ),
                        block.span,
                    );
                } else {
                    seen.push(condition);
                }
            }
            check_alternate(&block.alternate, seen, ctx);
        }
    }
}
