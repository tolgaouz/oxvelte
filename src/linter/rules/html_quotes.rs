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
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        let prefer = opts
            .and_then(|o| o.get("prefer"))
            .and_then(|v| v.as_str())
            .unwrap_or("double");

        let prefer_char = if prefer == "single" { '\'' } else { '"' };

        let dynamic = opts.and_then(|o| o.get("dynamic"));
        let dynamic_quoted = dynamic.and_then(|d| d.get("quoted"))
            .and_then(|v| v.as_bool()).unwrap_or(false);
        let avoid_invalid_unquoted = dynamic.and_then(|d| d.get("avoidInvalidUnquotedInHTML"))
            .and_then(|v| v.as_bool()).unwrap_or(false);

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

                            if let Some(eq_pos) = attr_src.find('=') {
                                let val_part = attr_src[eq_pos + 1..].trim();

                                match value {
                                    AttributeValue::Static(_) | AttributeValue::Concat(_) => {
                                        let is_correct = val_part.starts_with(prefer_char);
                                        if !is_correct {
                                            ctx.diagnostic(expected_msg,
                                                *span);
                                        }
                                    }
                                    AttributeValue::Expression(_) => {
                                        check_dynamic_quotes(val_part, dynamic_quoted, avoid_invalid_unquoted, prefer_char, expected_msg, unexpected_msg, *span, ctx);
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
                                check_dynamic_quotes(val_part, dynamic_quoted, avoid_invalid_unquoted, prefer_char, expected_msg, unexpected_msg, *span, ctx);
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

fn check_dynamic_quotes(
    val_part: &str, dynamic_quoted: bool, avoid_invalid_unquoted: bool,
    prefer_char: char, expected_msg: &str, unexpected_msg: &str,
    span: Span, ctx: &mut LintContext,
) {
    if dynamic_quoted {
        if !val_part.starts_with(prefer_char) {
            ctx.diagnostic(expected_msg, span);
        }
    } else {
        if val_part.starts_with('"') || val_part.starts_with('\'') {
            ctx.diagnostic(unexpected_msg, span);
        } else if avoid_invalid_unquoted
            && (val_part.contains('>') || val_part.contains('<') || val_part.contains('=') || val_part.contains('`')) {
            ctx.diagnostic(expected_msg, span);
        }
    }
}
