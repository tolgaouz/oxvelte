//! `svelte/valid-each-key` — enforce that each blocks with key use a unique identifier.
//! ⭐ Recommended

use crate::ast::TemplateNode;
use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::parser::expression::parse_template_expression;
use oxc::allocator::Allocator;
use oxc::ast::AstKind;
use oxc::semantic::SemanticBuilder;

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
            let TemplateNode::EachBlock(block) = node else {
                return;
            };
            let Some(key) = &block.key else { return };
            let key = key.trim();
            let mut iter_vars = extract_iter_vars(&block.context);
            if let Some(index) = &block.index {
                iter_vars.push(index.trim().to_string());
            }
            let uses_var = key_references_each_var(key, &iter_vars);
            if !uses_var {
                ctx.diagnostic(
                    "Expected key to use the variables which are defined by the `{#each}` block.",
                    block.span,
                );
            }
        });
    }
}

fn extract_iter_vars(context: &str) -> Vec<String> {
    let trimmed = context.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        trimmed[1..trimmed.len().saturating_sub(1)]
            .split(',')
            .map(|s| {
                let s = s.trim();
                s.find(':')
                    .map(|p| s[p + 1..].trim())
                    .unwrap_or(s.strip_prefix("...").unwrap_or(s))
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![trimmed.to_string()]
    }
}

fn key_references_each_var(key: &str, vars: &[String]) -> bool {
    if vars.iter().all(|var| var.is_empty()) {
        return false;
    }

    let alloc = Allocator::default();
    let parsed = parse_template_expression(key, &alloc);
    if !parsed.errors.is_empty() {
        return false;
    }
    let semantic = SemanticBuilder::new().build(&parsed.program).semantic;
    let references_each_var = semantic.nodes().iter().any(|node| {
        let AstKind::IdentifierReference(id) = node.kind() else {
            return false;
        };
        vars.iter().any(|var| id.name == var.as_str())
    });
    references_each_var
}
