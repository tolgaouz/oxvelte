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
        // Parse config options
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        let dynamic_quoted = opts
            .and_then(|o| o.get("dynamic"))
            .and_then(|d| d.get("quoted"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let avoid_invalid_unquoted = opts
            .and_then(|o| o.get("dynamic"))
            .and_then(|d| d.get("avoidInvalidUnquotedInHTML"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name: _, value, span } => {
                            let start = span.start as usize;
                            let end = span.end as usize;
                            if end > ctx.source.len() { continue; }
                            let attr_src = &ctx.source[start..end];

                            // Find the `=` to get the value portion
                            if let Some(eq_pos) = attr_src.find('=') {
                                let val_part = attr_src[eq_pos + 1..].trim();

                                match value {
                                    AttributeValue::Static(_) => {
                                        // Unquoted or single-quoted static values
                                        if !val_part.starts_with('"') {
                                            ctx.diagnostic(
                                                "Expected to be enclosed by double quotes.",
                                                Span::new(span.start, span.end),
                                            );
                                        }
                                    }
                                    AttributeValue::Expression(_) | AttributeValue::Concat(_) => {
                                        if dynamic_quoted {
                                            // Check if the dynamic value is enclosed in quotes
                                            if !val_part.starts_with('"') {
                                                ctx.diagnostic(
                                                    "Expected to be enclosed by double quotes.",
                                                    Span::new(span.start, span.end),
                                                );
                                            }
                                        } else if avoid_invalid_unquoted && !val_part.starts_with('"') {
                                            // Check if the unquoted value contains chars invalid in HTML
                                            if val_part.contains('>') || val_part.contains('<')
                                                || val_part.contains('=') || val_part.contains('`')
                                            {
                                                ctx.diagnostic(
                                                    "Expected to be enclosed by double quotes.",
                                                    Span::new(span.start, span.end),
                                                );
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Attribute::Directive { span, .. } => {
                            let start = span.start as usize;
                            let end = span.end as usize;
                            if end > ctx.source.len() { continue; }
                            let attr_src = &ctx.source[start..end];
                            if let Some(eq_pos) = attr_src.find('=') {
                                let val_part = attr_src[eq_pos + 1..].trim();
                                if dynamic_quoted {
                                    // Check directive values like bind:value={(text)}
                                    if val_part.starts_with('{') && !val_part.starts_with("\"") {
                                        ctx.diagnostic(
                                            "Expected to be enclosed by double quotes.",
                                            Span::new(span.start, span.end),
                                        );
                                    }
                                } else if avoid_invalid_unquoted && !val_part.starts_with('"') {
                                    if val_part.contains('>') || val_part.contains('<')
                                        || val_part.contains('=') || val_part.contains('`')
                                    {
                                        ctx.diagnostic(
                                            "Expected to be enclosed by double quotes.",
                                            Span::new(span.start, span.end),
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}
