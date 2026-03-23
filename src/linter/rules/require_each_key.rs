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
                    // Skip if no context variable (Svelte 5 comma syntax with only index)
                    if block.context.trim().is_empty() {
                        return;
                    }
                    // Skip Svelte 5 object literal expressions like { length: N }
                    // but only when there's no `as` context (Svelte 5 comma syntax).
                    // When `as` is used (context is non-empty), object literals like
                    // `{ length: 20 } as _` should still be flagged.
                    let expr = block.expression.trim();
                    if expr.starts_with('{') {
                        // Find the matching closing brace
                        let mut depth = 0i32;
                        let mut is_obj = false;
                        for (i, ch) in expr.char_indices() {
                            match ch {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        let rest = expr[i+1..].trim();
                                        if rest.is_empty() || rest.starts_with(',') {
                                            is_obj = true;
                                        }
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        if is_obj { return; }
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
