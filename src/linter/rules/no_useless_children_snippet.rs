//! `svelte/no-useless-children-snippet` — disallow useless children snippets.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoUselessChildrenSnippet;

impl Rule for NoUselessChildrenSnippet {
    fn name(&self) -> &'static str {
        "svelte/no-useless-children-snippet"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Check if element is a component (capitalized name)
                if el.name.starts_with(|c: char| c.is_uppercase()) {
                    for child in &el.children {
                        if let TemplateNode::SnippetBlock(snippet) = child {
                            if snippet.name == "children" && snippet.params.trim().is_empty() {
                                // A {#snippet children()} without parameters is useless
                                ctx.diagnostic(
                                    "Useless `{#snippet children()}` — place content directly as children of the component.",
                                    snippet.span,
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}
