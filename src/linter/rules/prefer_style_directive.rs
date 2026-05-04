//! `svelte/prefer-style-directive` — prefer `style:` directives over `style` attributes.
//! 🔧 Fixable

use crate::ast::{Attribute, AttributeValue, TemplateNode};
use crate::linter::{walk_template_nodes, LintContext, Rule};

pub struct PreferStyleDirective;

impl Rule for PreferStyleDirective {
    fn name(&self) -> &'static str {
        "svelte/prefer-style-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            // Apply only to elements that render a real DOM node — `<svelte:element>`
            // does, the other `svelte:*` specials don't, and components have their
            // own style mechanism.
            match el.kind() {
                crate::ast::ElementKind::Html => {}
                crate::ast::ElementKind::SvelteSpecial(crate::ast::SvelteSpecial::Element) => {}
                _ => return,
            }
            for attr in &el.attributes {
                if let Attribute::NormalAttribute { name, value, span } = attr {
                    if name != "style" {
                        continue;
                    }
                    match value {
                        AttributeValue::Static(s) => {
                            for _ in 0..s.split(';').filter(|d| d.trim().contains(':')).count() {
                                ctx.diagnostic("Can use style directives instead.", *span);
                            }
                        }
                        AttributeValue::Concat(parts) => {
                            use crate::ast::AttributeValuePart::*;
                            if parts.iter().any(|p| match p {
                                Static(s) => s.contains(':'),
                                Expression(e) => e.contains(':'),
                            }) {
                                ctx.diagnostic("Can use style directives instead.", *span);
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}
