//! `svelte/no-spaces-around-equal-signs-in-attribute` — disallow spaces around `=` in attributes.
//! 🔧 Fixable

use crate::ast::{Attribute, TemplateNode};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};

pub struct NoSpacesAroundEqualSignsInAttribute;

impl Rule for NoSpacesAroundEqualSignsInAttribute {
    fn name(&self) -> &'static str {
        "svelte/no-spaces-around-equal-signs-in-attribute"
    }
    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            for (attr, meta) in el.attributes.iter().zip(&el.attribute_meta) {
                match attr {
                    Attribute::NormalAttribute { .. } | Attribute::Directive { .. } => {}
                    Attribute::Spread { .. } => continue,
                }
                let Some(eq_span) = meta.equals_span else {
                    continue;
                };
                if eq_span.end.saturating_sub(eq_span.start) > 1 {
                    ctx.diagnostic_with_fix(
                        "Unexpected spaces found around equal signs.",
                        eq_span,
                        Fix {
                            span: eq_span,
                            replacement: "=".to_string(),
                        },
                    );
                }
            }
        });
    }
}
