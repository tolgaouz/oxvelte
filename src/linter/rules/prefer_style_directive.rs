//! `svelte/prefer-style-directive` — prefer `style:` directives over `style` attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};

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
                // Skip special elements and components (style: directives only work on HTML elements)
                if el.name.starts_with("svelte:")
                    || el.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                {
                    return;
                }
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span } = attr {
                        if name == "style" {
                            // Only flag static style attributes with parseable CSS declarations
                            // Don't flag expression-only styles (variables), shorthand, or concat
                            match value {
                                AttributeValue::Static(s) => {
                                    // Only flag if it contains actual CSS declarations
                                    if s.contains(':') {
                                        ctx.diagnostic(
                                            "Prefer using `style:property={value}` directive instead of the `style` attribute.",
                                            *span,
                                        );
                                    }
                                }
                                _ => {
                                    // Expression or concat — can't reliably convert, skip
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
