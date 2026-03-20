//! `svelte/html-quotes` — enforce consistent use of double or single quotes in attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};
use oxc::span::Span;

pub struct HtmlQuotes;

impl Rule for HtmlQuotes {
    fn name(&self) -> &'static str {
        "svelte/html-quotes"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name: _, value: AttributeValue::Static(_), span } = attr {
                        // Check the source text at the span to see what quote character is used.
                        let start = span.start as usize;
                        let end = span.end as usize;
                        if end <= ctx.source.len() {
                            let attr_src = &ctx.source[start..end];
                            if attr_src.contains('\'') && !attr_src.contains('"') {
                                ctx.diagnostic(
                                    "Use double quotes for attribute values.",
                                    Span::new(span.start, span.end),
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}
