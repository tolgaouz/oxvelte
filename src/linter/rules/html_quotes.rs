//! `svelte/html-quotes` — enforce consistent use of double or single quotes in attributes.
//! 🔧 Fixable

use crate::ast::{
    Attribute, AttributeMeta, AttributeQuote, AttributeValue, AttributeValuePart, DirectiveKind,
    TemplateNode,
};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
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
        let opts = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        let prefer = opts
            .and_then(|o| o.get("prefer"))
            .and_then(|v| v.as_str())
            .unwrap_or("double");

        let prefer_quote = if prefer == "single" {
            Quote::Single
        } else {
            Quote::Double
        };

        let dynamic = opts.and_then(|o| o.get("dynamic"));
        let dynamic_quoted = dynamic
            .and_then(|d| d.get("quoted"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let avoid_invalid_unquoted = dynamic
            .and_then(|d| d.get("avoidInvalidUnquotedInHTML"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for (attr, meta) in el.attributes.iter().zip(&el.attribute_meta) {
                    match attr {
                        Attribute::NormalAttribute { .. } | Attribute::Directive { .. } => {}
                        Attribute::Spread { .. } => continue,
                    }
                    if meta.equals_span.is_none() {
                        continue;
                    }

                    match attr {
                        Attribute::NormalAttribute {
                            name: _,
                            value,
                            span: _,
                        } => {
                            if single_mustache_span(value, meta).is_some() {
                                check_dynamic_quotes(
                                    ctx,
                                    meta,
                                    dynamic_quoted,
                                    avoid_invalid_unquoted,
                                    prefer_quote,
                                );
                            } else if !matches!(value, AttributeValue::True) {
                                verify_quote(ctx, prefer_quote, meta);
                            }
                        }
                        Attribute::Directive { kind, value, .. } => {
                            if matches!(kind, DirectiveKind::StyleDirective)
                                && single_mustache_span(value, meta).is_none()
                            {
                                if !matches!(value, AttributeValue::True) {
                                    verify_quote(ctx, prefer_quote, meta);
                                }
                            } else if !matches!(value, AttributeValue::True) {
                                check_dynamic_quotes(
                                    ctx,
                                    meta,
                                    dynamic_quoted,
                                    avoid_invalid_unquoted,
                                    prefer_quote,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    Double,
    Single,
    Unquoted,
}

impl Quote {
    fn char(self) -> Option<char> {
        match self {
            Quote::Double => Some('"'),
            Quote::Single => Some('\''),
            Quote::Unquoted => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Quote::Double => "double quotes",
            Quote::Single => "single quotes",
            Quote::Unquoted => "unquoted",
        }
    }
}

struct QuoteAndRange {
    quote: Quote,
    range: Span,
}

fn check_dynamic_quotes(
    ctx: &mut LintContext<'_>,
    meta: &AttributeMeta<'_>,
    dynamic_quoted: bool,
    avoid_invalid_unquoted: bool,
    prefer_quote: Quote,
) {
    let dynamic_quote = if dynamic_quoted {
        prefer_quote
    } else {
        Quote::Unquoted
    };
    let expected = if avoid_invalid_unquoted
        && single_mustache_text(ctx.source, meta).is_some_and(|text| !can_be_unquoted_in_html(text))
    {
        prefer_quote
    } else {
        dynamic_quote
    };

    verify_quote(ctx, expected, meta);
}

fn verify_quote(ctx: &mut LintContext<'_>, prefer: Quote, meta: &AttributeMeta<'_>) {
    let Some(quote_range) = quote_and_range(ctx.source, meta) else {
        return;
    };
    if quote_range.quote == prefer {
        return;
    }

    let content = quote_content(ctx.source, &quote_range);
    let mut expected = prefer;
    let message;

    if quote_range.quote != Quote::Unquoted {
        if expected == Quote::Unquoted {
            message = "Unexpected to be enclosed by any quotes.".to_string();
        } else if expected
            .char()
            .is_some_and(|expected_char| content.contains(expected_char))
        {
            return;
        } else {
            message = format!("Expected to be enclosed by {}.", expected.name());
        }
    } else {
        let has_double = content.contains('"');
        let has_single = content.contains('\'');
        if has_double && has_single {
            return;
        }
        if has_double && expected == Quote::Double {
            expected = Quote::Single;
            message = "Expected to be enclosed by quotes.".to_string();
        } else if has_single && expected == Quote::Single {
            expected = Quote::Double;
            message = "Expected to be enclosed by quotes.".to_string();
        } else {
            message = format!("Expected to be enclosed by {}.", expected.name());
        }
    }

    ctx.diagnostic_with_fix(
        message,
        quote_range.range,
        Fix {
            span: quote_range.range,
            replacement: fixed_quote_text(content, expected),
        },
    );
}

fn single_mustache_span(value: &AttributeValue, meta: &AttributeMeta<'_>) -> Option<Span> {
    match value {
        AttributeValue::Expression(_) => meta.mustache_span.or(meta.value_span),
        AttributeValue::Concat(parts) => {
            if matches!(parts.as_slice(), [AttributeValuePart::Expression(_)]) {
                meta.parts
                    .first()
                    .and_then(|part| part.mustache_span)
                    .or(meta.mustache_span)
                    .or(meta.value_span)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn single_mustache_text<'a>(source: &'a str, meta: &AttributeMeta<'_>) -> Option<&'a str> {
    let span = meta
        .mustache_span
        .or_else(|| meta.parts.first().and_then(|part| part.mustache_span))
        .or(meta.value_span)?;
    source.get(span.start as usize..span.end as usize)
}

fn can_be_unquoted_in_html(text: &str) -> bool {
    !text
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '=' | '>' | '`'))
}

fn quote_and_range(_source: &str, meta: &AttributeMeta<'_>) -> Option<QuoteAndRange> {
    let value_span = meta.value_span?;
    match meta.quote {
        Some(AttributeQuote::Double) => Some(QuoteAndRange {
            quote: Quote::Double,
            range: meta.value_full_span?,
        }),
        Some(AttributeQuote::Single) => Some(QuoteAndRange {
            quote: Quote::Single,
            range: meta.value_full_span?,
        }),
        None => {
            let range = meta.value_full_span.unwrap_or(value_span);
            Some(QuoteAndRange {
                quote: Quote::Unquoted,
                range,
            })
        }
    }
}

fn quote_content<'a>(source: &'a str, quote_range: &QuoteAndRange) -> &'a str {
    let mut start = quote_range.range.start as usize;
    let mut end = quote_range.range.end as usize;
    if quote_range.quote != Quote::Unquoted {
        start += 1;
        end = end.saturating_sub(1);
    }
    source.get(start..end).unwrap_or("")
}

fn fixed_quote_text(content: &str, expected: Quote) -> String {
    match expected.char() {
        Some(quote) => format!("{quote}{content}{quote}"),
        None => content.to_string(),
    }
}
