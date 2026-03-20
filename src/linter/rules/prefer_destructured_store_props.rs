//! `svelte/prefer-destructured-store-props` — prefer destructuring store props.
//! 💡 Has suggestion

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct PreferDestructuredStoreProps;

impl Rule for PreferDestructuredStoreProps {
    fn name(&self) -> &'static str {
        "svelte/prefer-destructured-store-props"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Flag template expressions like `$store.prop` suggesting destructuring.
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::MustacheTag(tag) = node {
                let expr = tag.expression.trim();
                if expr.starts_with('$') && expr.contains('.') && !expr.contains('(') {
                    ctx.diagnostic(
                        format!(
                            "Prefer destructuring `{}` from the store rather than accessing it directly.",
                            expr
                        ),
                        tag.span,
                    );
                }
            }
        });
    }
}
