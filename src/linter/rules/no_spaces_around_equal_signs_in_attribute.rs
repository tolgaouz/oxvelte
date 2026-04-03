//! `svelte/no-spaces-around-equal-signs-in-attribute` — disallow spaces around `=` in attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, TemplateNode};

pub struct NoSpacesAroundEqualSignsInAttribute;

impl Rule for NoSpacesAroundEqualSignsInAttribute {
    fn name(&self) -> &'static str { "svelte/no-spaces-around-equal-signs-in-attribute" }
    fn is_fixable(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            for attr in &el.attributes {
                let span = match attr {
                    Attribute::NormalAttribute { span, .. } | Attribute::Directive { span, .. } => *span,
                    Attribute::Spread { .. } => continue,
                };
                let text = &ctx.source[span.start as usize..span.end as usize];
                if let Some(eq) = text.find('=') {
                    if text[..eq].ends_with(|c: char| c.is_whitespace()) || text[eq + 1..].starts_with(|c: char| c.is_whitespace()) {
                        ctx.diagnostic("Unexpected spaces found around equal signs.", span);
                    }
                }
            }
        });
    }
}
