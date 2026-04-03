//! `svelte/shorthand-attribute` — enforce use of shorthand syntax for attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};

pub struct ShorthandAttribute;

impl Rule for ShorthandAttribute {
    fn name(&self) -> &'static str {
        "svelte/shorthand-attribute"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let prefer_never = ctx.config.options.as_ref()
            .and_then(|v| v.as_array()).and_then(|arr| arr.first())
            .and_then(|v| v.get("prefer")).and_then(|v| v.as_str()) == Some("never");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            for attr in &el.attributes {
                if let Attribute::NormalAttribute { name, value: AttributeValue::Expression(expr), span } = attr {
                    if name != expr.trim() { continue; }
                    let src = &ctx.source[span.start as usize..span.end as usize];
                    if prefer_never && src.starts_with('{') { ctx.diagnostic("Expected regular attribute syntax.", *span); }
                    else if !prefer_never && !src.starts_with('{') { ctx.diagnostic("Expected shorthand attribute.", *span); }
                }
            }
        });
    }
}
