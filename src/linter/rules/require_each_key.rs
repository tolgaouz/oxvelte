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
                    // Skip Svelte 5 object literal expressions like { length: N }
                    // which represent fixed-length ranges and don't need keys.
                    let expr = block.expression.trim();
                    // Direct object literal: { length: 8 }
                    if expr.starts_with('{') && expr.ends_with('}') {
                        return;
                    }
                    // Svelte 5 comma syntax: { length: 8 }, rank — expression includes the context
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
                                        // Check if rest is just ", identifier" (Svelte 5 context)
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
