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
        // Config: { "prefer": "always" | "never" }, default "always"
        let prefer_never = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("prefer"))
            .and_then(|v| v.as_str())
            .map(|s| s == "never")
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value: AttributeValue::Expression(expr), span } = attr {
                        if name == expr.trim() {
                            let src = &ctx.source[span.start as usize..span.end as usize];
                            if prefer_never {
                                // "never" mode: flag shorthand usage {name}, expect name={name}
                                if src.starts_with('{') {
                                    ctx.diagnostic(
                                        "Expected regular attribute syntax.",
                                        *span,
                                    );
                                }
                            } else {
                                // "always" mode (default): flag longhand, expect {name}
                                if !src.starts_with('{') {
                                    ctx.diagnostic(
                                        "Expected shorthand attribute.",
                                        *span,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
