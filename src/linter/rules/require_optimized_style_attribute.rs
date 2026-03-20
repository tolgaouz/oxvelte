//! `svelte/require-optimized-style-attribute` — require use of optimized style attribute syntax.
//!
//! Checks for `style` attributes that use string concatenation or template literals
//! instead of the object/directive form that Svelte can optimize.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};

pub struct RequireOptimizedStyleAttribute;

impl Rule for RequireOptimizedStyleAttribute {
    fn name(&self) -> &'static str {
        "svelte/require-optimized-style-attribute"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span } = attr {
                        if name == "style" {
                            match value {
                                AttributeValue::Concat(_) => {
                                    ctx.diagnostic(
                                        "Use `style:property={value}` directives instead of concatenated style strings for better performance.",
                                        *span,
                                    );
                                }
                                AttributeValue::Expression(_) => {
                                    ctx.diagnostic(
                                        "Use `style:property={value}` directives instead of a dynamic style expression for better performance.",
                                        *span,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        });
    }
}
