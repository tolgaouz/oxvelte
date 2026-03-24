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
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::MustacheTag(tag) = node {
                let expr = tag.expression.trim();
                if !expr.starts_with('$') || expr.contains('(') {
                    return;
                }

                // Check for dot access: $store.prop
                if let Some(dot_pos) = expr.find('.') {
                    let store = &expr[..dot_pos];
                    let property = &expr[dot_pos + 1..];
                    ctx.diagnostic(
                        format!(
                            "Destructure {} from {} for better change tracking & fewer redraws",
                            property, store
                        ),
                        tag.span,
                    );
                    return;
                }

                // Check for bracket access: $store[prop] or $store['prop']
                // But NOT $store[`template${var}`] (computed access)
                if let Some(bracket_pos) = expr.find('[') {
                    let store = &expr[..bracket_pos];
                    let inner = &expr[bracket_pos + 1..];
                    if let Some(close) = inner.rfind(']') {
                        let key = inner[..close].trim();
                        // Only flag simple identifiers and string literals
                        let is_simple = key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            || (key.starts_with('\'') && key.ends_with('\''))
                            || (key.starts_with('"') && key.ends_with('"'));
                        if is_simple {
                            ctx.diagnostic(
                                format!(
                                    "Destructure {} from {} for better change tracking & fewer redraws",
                                    key, store
                                ),
                                tag.span,
                            );
                        }
                    }
                }
            }
        });
    }
}
