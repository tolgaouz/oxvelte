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
                    let key_trimmed = key.trim();

                    // Key should reference a property of the iteration variable(s).
                    // Extract the iteration variable names from the context pattern.
                    let iter_vars = extract_iter_vars(&block.context);

                    // Check if the key uses the iteration variable itself
                    if key_trimmed == block.context.trim() {
                        ctx.diagnostic(
                            format!(
                                "Using the iteration variable '{}' directly as the key is not recommended. Use a unique identifier like '{}.id'.",
                                block.context, block.context
                            ),
                            block.span,
                        );
                        return;
                    }

                    // Check if the key expression references any of the iteration variables
                    let uses_iter_var = iter_vars.iter().any(|var| {
                        key_contains_var(key_trimmed, var)
                    });

                    // Also check if the key references the index variable
                    let uses_index = if let Some(ref idx) = block.index {
                        key_contains_var(key_trimmed, idx)
                    } else { false };

                    if !uses_iter_var && !uses_index {
                        ctx.diagnostic(
                            "Expected key to use the variables which are defined by the `{#each}` block.",
                            block.span,
                        );
                    }
                }
            }
        });
    }
}

/// Extract variable names from an each context pattern.
/// E.g., "item" -> ["item"], "{ id, name }" -> ["id", "name"], "[a, b]" -> ["a", "b"]
fn extract_iter_vars(context: &str) -> Vec<String> {
    let trimmed = context.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Destructured pattern
        let inner = &trimmed[1..trimmed.len().saturating_sub(1)];
        inner.split(',')
            .map(|s| {
                let s = s.trim();
                // Handle renaming: `original: alias`
                if let Some(colon_pos) = s.find(':') {
                    s[colon_pos + 1..].trim().to_string()
                } else {
                    // Handle rest: `...rest`
                    s.strip_prefix("...").unwrap_or(s).to_string()
                }
            })
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![trimmed.to_string()]
    }
}

/// Check if a key expression contains a reference to a variable.
fn key_contains_var(key: &str, var: &str) -> bool {
    if var.is_empty() { return false; }
    // Check for whole-word match: var must not be preceded/followed by alphanumeric/underscore
    let mut search_from = 0;
    while let Some(pos) = key[search_from..].find(var) {
        let abs = search_from + pos;
        let before_ok = abs == 0 || !key.as_bytes()[abs - 1].is_ascii_alphanumeric() && key.as_bytes()[abs - 1] != b'_';
        let after_pos = abs + var.len();
        let after_ok = after_pos >= key.len() || (!key.as_bytes()[after_pos].is_ascii_alphanumeric() && key.as_bytes()[after_pos] != b'_');
        if before_ok && after_ok {
            return true;
        }
        search_from = abs + 1;
    }
    false
}
