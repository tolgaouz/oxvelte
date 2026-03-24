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

        // Read `prefer` option — default "double"
        let prefer = opts
            .and_then(|o| o.get("prefer"))
            .and_then(|v| v.as_str())
            .unwrap_or("double");

        let prefer_char = if prefer == "single" { '\'' } else { '"' };

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

        let expected_msg = if prefer == "single" {
            "Expected to be enclosed by single quotes."
        } else {
            "Expected to be enclosed by double quotes."
        };
        let unexpected_msg = "Unexpected to be enclosed by any quotes.";

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
                                    AttributeValue::Static(_) | AttributeValue::Concat(_) => {
                                        // Static or mixed (concat) value: must use the preferred quote char
                                        let is_correct = val_part.starts_with(prefer_char);
                                        if !is_correct {
                                            ctx.diagnostic(
                                                expected_msg,
                                                Span::new(span.start, span.end),
                                            );
                                        }
                                    }
                                    AttributeValue::Expression(_) => {
                                        // Pure dynamic expression
                                        if dynamic_quoted {
                                            // dynamic.quoted = true: dynamic values should be quoted with prefer char
                                            if !val_part.starts_with(prefer_char) {
                                                ctx.diagnostic(
                                                    expected_msg,
                                                    Span::new(span.start, span.end),
                                                );
                                            }
                                        } else {
                                            // dynamic.quoted = false (default): dynamic values should NOT be quoted
                                            let is_quoted = val_part.starts_with('"') || val_part.starts_with('\'');
                                            if is_quoted {
                                                ctx.diagnostic(
                                                    unexpected_msg,
                                                    Span::new(span.start, span.end),
                                                );
                                            } else if avoid_invalid_unquoted {
                                                // Check if the unquoted value contains chars invalid in HTML
                                                if val_part.contains('>') || val_part.contains('<')
                                                    || val_part.contains('=') || val_part.contains('`')
                                                {
                                                    ctx.diagnostic(
                                                        expected_msg,
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
                        Attribute::Directive { span, .. } => {
                            let start = span.start as usize;
                            let end = span.end as usize;
                            if end > ctx.source.len() { continue; }
                            let attr_src = &ctx.source[start..end];
                            if let Some(eq_pos) = attr_src.find('=') {
                                let val_part = attr_src[eq_pos + 1..].trim();
                                if dynamic_quoted {
                                    // dynamic.quoted = true: directive values should be quoted with preferred char
                                    if val_part.starts_with('{') && !val_part.starts_with(prefer_char) {
                                        ctx.diagnostic(
                                            expected_msg,
                                            Span::new(span.start, span.end),
                                        );
                                    }
                                } else {
                                    // dynamic.quoted = false (default): directive values should NOT be quoted
                                    let is_quoted = val_part.starts_with('"') || val_part.starts_with('\'');
                                    if is_quoted {
                                        ctx.diagnostic(
                                            unexpected_msg,
                                            Span::new(span.start, span.end),
                                        );
                                    } else if avoid_invalid_unquoted {
                                        if val_part.contains('>') || val_part.contains('<')
                                            || val_part.contains('=') || val_part.contains('`')
                                        {
                                            ctx.diagnostic(
                                                expected_msg,
                                                Span::new(span.start, span.end),
                                            );
                                        }
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
