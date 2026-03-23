//! `svelte/no-spaces-around-equal-signs-in-attribute` — disallow spaces around `=` in attributes.
//! 🔧 Fixable
//!
//! Checks for patterns like `name = "value"` or `name ="value"` or `name= "value"`
//! in element attributes via simple source text scanning.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, TemplateNode};

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
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, span, .. } = attr {
                        // Inspect the source text of the attribute for spaces around `=`
                        let start = span.start as usize;
                        let end = span.end as usize;
                        if end <= ctx.source.len() {
                            let attr_text = &ctx.source[start..end];
                            if let Some(eq_idx) = attr_text.find('=') {
                                let before_eq = &attr_text[..eq_idx];
                                let after_eq = &attr_text[eq_idx + 1..];
                                if before_eq.ends_with(' ') || after_eq.starts_with(' ') {
                                    ctx.diagnostic(
                                        format!("Unexpected space(s) around `=` in attribute `{name}`."),
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
