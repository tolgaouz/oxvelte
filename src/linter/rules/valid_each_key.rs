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
            let TemplateNode::EachBlock(block) = node else { return };
            let Some(key) = &block.key else { return };
            let key = key.trim();
            let iter_vars = extract_iter_vars(&block.context);
            let uses_var = iter_vars.iter().any(|v| key_contains_var(key, v))
                || block.index.as_ref().map_or(false, |idx| key_contains_var(key, idx));
            if !uses_var {
                ctx.diagnostic("Expected key to use the variables which are defined by the `{#each}` block.", block.span);
            }
        });
    }
}

fn extract_iter_vars(context: &str) -> Vec<String> {
    let trimmed = context.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        trimmed[1..trimmed.len().saturating_sub(1)].split(',')
            .map(|s| { let s = s.trim(); s.find(':').map(|p| s[p+1..].trim()).unwrap_or(s.strip_prefix("...").unwrap_or(s)).to_string() })
            .filter(|s| !s.is_empty()).collect()
    } else { vec![trimmed.to_string()] }
}

fn key_contains_var(key: &str, var: &str) -> bool {
    if var.is_empty() { return false; }
    let is_boundary = |b: u8| !b.is_ascii_alphanumeric() && b != b'_';
    let mut from = 0;
    while let Some(pos) = key[from..].find(var) {
        let abs = from + pos;
        let end = abs + var.len();
        if (abs == 0 || is_boundary(key.as_bytes()[abs - 1])) && (end >= key.len() || is_boundary(key.as_bytes()[end])) {
            return true;
        }
        from = abs + 1;
    }
    false
}
