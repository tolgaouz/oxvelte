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
            let TemplateNode::MustacheTag(tag) = node else { return };
            let expr = tag.expression.trim();
            if !expr.starts_with('$') || expr.contains('(') { return; }
            let msg = |prop, store| format!("Destructure {} from {} for better change tracking & fewer redraws", prop, store);

            if let Some(dot) = expr.find('.') {
                let store = &expr[..dot];
                if store.starts_with("$$") { return; }
                let prop = expr[dot + 1..].split('.').next().unwrap_or("");
                ctx.diagnostic(msg(prop, store), tag.span);
            } else if let Some(br) = expr.find('[') {
                let store = &expr[..br];
                if let Some(close) = expr[br + 1..].rfind(']') {
                    let key = expr[br + 1..br + 1 + close].trim();
                    let is_simple = key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                        || (key.starts_with('\'') && key.ends_with('\''))
                        || (key.starts_with('"') && key.ends_with('"'));
                    if is_simple { ctx.diagnostic(msg(key, store), tag.span); }
                }
            }
        });
    }
}
