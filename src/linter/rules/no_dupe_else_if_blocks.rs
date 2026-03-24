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
                let mut seen_conditions: Vec<Vec<Vec<String>>> = vec![split_or_and(&block.test)];
                check_alternate(&block.alternate, &mut seen_conditions, ctx);
            }
        });
    }
}

/// Split a condition string by `||` (top-level), then each OR branch by `&&`.
/// Returns Vec<Vec<String>>: outer = OR branches, inner = AND operands.
fn split_or_and(cond: &str) -> Vec<Vec<String>> {
    split_top_level(cond.trim(), "||")
        .into_iter()
        .map(|branch| {
            split_top_level(&branch, "&&")
                .into_iter()
                .map(|s| s.trim().to_string())
                .collect()
        })
        .collect()
}

/// Split an expression at a top-level operator (||, &&), respecting parentheses.
fn split_top_level(s: &str, op: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let op_bytes = op.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => { depth += 1; i += 1; }
            b')' => { depth -= 1; i += 1; }
            _ if depth == 0 && i + op_bytes.len() <= bytes.len() && &bytes[i..i + op_bytes.len()] == op_bytes => {
                parts.push(s[start..i].trim().to_string());
                i += op_bytes.len();
                start = i;
            }
            _ => { i += 1; }
        }
    }
    parts.push(s[start..].trim().to_string());
    parts
}

/// Check if condition `current` is a subset of (fully covered by) any previously seen condition.
/// A condition is "covered" if every OR branch of `current` is a subset of some OR branch of some `prev`.
fn is_covered(current: &[Vec<String>], seen: &[Vec<Vec<String>>]) -> bool {
    // For each OR branch of `current`, check if it's a subset of some OR branch of some `prev`
    current.iter().all(|cur_and_operands| {
        seen.iter().any(|prev| {
            prev.iter().any(|prev_and_operands| {
                // cur_and_operands is a subset of prev_and_operands if
                // every AND operand in prev is present in cur
                prev_and_operands.iter().all(|p| cur_and_operands.contains(p))
            })
        })
    })
}

fn check_alternate(
    alternate: &Option<Box<crate::ast::TemplateNode>>,
    seen: &mut Vec<Vec<Vec<String>>>,
    ctx: &mut LintContext<'_>,
) {
    if let Some(alt) = alternate {
        if let TemplateNode::IfBlock(block) = alt.as_ref() {
            let condition = block.test.trim().to_string();
            if !condition.is_empty() {
                let parsed = split_or_and(&condition);
                if is_covered(&parsed, seen) {
                    ctx.diagnostic(
                        "This branch can never execute. Its condition is a duplicate or covered by previous conditions in the `{#if}` / `{:else if}` chain.",
                        block.span,
                    );
                }
                seen.push(parsed);
            }
            check_alternate(&block.alternate, seen, ctx);
        }
    }
}
