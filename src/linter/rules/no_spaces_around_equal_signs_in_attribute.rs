//! `svelte/no-spaces-around-equal-signs-in-attribute` — disallow spaces around `=` in attributes.
//! 🔧 Fixable
//!
//! Checks for patterns like `name = "value"` or `name ="value"` or `name= "value"`
//! in element attributes and directives via simple source text scanning.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, TemplateNode};

pub struct NoSpacesAroundEqualSignsInAttribute;

const MESSAGE: &str = "Unexpected spaces found around equal signs.";

/// Checks the source text of an attribute for whitespace immediately before or after `=`.
/// Returns `true` if a space-padded `=` is found.
fn has_spaces_around_eq(text: &str) -> bool {
    if let Some(eq_idx) = text.find('=') {
        let before_eq = &text[..eq_idx];
        let after_eq = &text[eq_idx + 1..];
        before_eq.ends_with(|c: char| c.is_whitespace())
            || after_eq.starts_with(|c: char| c.is_whitespace())
    } else {
        false
    }
}

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
                    match attr {
                        Attribute::NormalAttribute { span, .. } => {
                            let start = span.start as usize;
                            let end = span.end as usize;
                            if end <= ctx.source.len() {
                                let attr_text = &ctx.source[start..end];
                                if has_spaces_around_eq(attr_text) {
                                    ctx.diagnostic(MESSAGE, *span);
                                }
                            }
                        }
                        Attribute::Directive { span, .. } => {
                            let start = span.start as usize;
                            let end = span.end as usize;
                            if end <= ctx.source.len() {
                                let attr_text = &ctx.source[start..end];
                                if has_spaces_around_eq(attr_text) {
                                    ctx.diagnostic(MESSAGE, *span);
                                }
                            }
                        }
                        Attribute::Spread { .. } => {}
                    }
                }
            }
        });
    }
}
