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
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value: AttributeValue::Expression(expr), span } = attr {
                        if name == expr.trim() {
                            // Check if source already uses shorthand form {name}
                            let src = &ctx.source[span.start as usize..span.end as usize];
                            if !src.starts_with('{') {
                                ctx.diagnostic(
                                    format!("Use shorthand `{{{}}}` instead of `{}={{{}}}`.", name, name, name),
                                    *span,
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}
