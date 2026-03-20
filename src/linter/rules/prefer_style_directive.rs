//! `svelte/prefer-style-directive` — prefer `style:` directives over `style` attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, TemplateNode};

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
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, span, .. } = attr {
                        if name == "style" {
                            ctx.diagnostic(
                                "Prefer using `style:property={value}` directive instead of the `style` attribute.",
                                *span,
                            );
                        }
                    }
                }
            }
        });
    }
}
