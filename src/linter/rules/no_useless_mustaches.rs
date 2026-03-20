//! `svelte/no-useless-mustaches` — disallow unnecessary mustache interpolations.
//! ⭐ Recommended, 🔧 Fixable

use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoUselessMustaches;

impl Rule for NoUselessMustaches {
    fn name(&self) -> &'static str {
        "svelte/no-useless-mustaches"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::MustacheTag(tag) = node {
                let expr = tag.expression.trim();
                // Check if expression is a simple string literal
                if (expr.starts_with('\'') && expr.ends_with('\''))
                    || (expr.starts_with('"') && expr.ends_with('"'))
                    || (expr.starts_with('`') && expr.ends_with('`') && !expr.contains("${"))
                {
                    let inner = &expr[1..expr.len() - 1];
                    ctx.diagnostic_with_fix(
                        "Unnecessary mustache interpolation around a string literal. Use the text directly.",
                        tag.span,
                        Fix {
                            span: tag.span,
                            replacement: inner.to_string(),
                        },
                    );
                }
            }
        });
    }
}
