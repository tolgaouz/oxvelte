//! `svelte/mustache-spacing` — enforce consistent spacing inside mustache braces `{ }`.
//! 🔧 Fixable
//!
//! Default: "never" — no spaces inside mustache braces: `{expr}`.

use crate::ast::{Attribute, AttributeMeta, TemplateNode};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use oxc::span::Span;
use std::collections::HashSet;

pub struct MustacheSpacing;

impl Rule for MustacheSpacing {
    fn name(&self) -> &'static str {
        "svelte/mustache-spacing"
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
        let text_always = opts
            .and_then(|v| v.get("textExpressions"))
            .and_then(|v| v.as_str())
            == Some("always");
        let attributes_always = opts
            .and_then(|v| v.get("attributesAndProps"))
            .and_then(|v| v.as_str())
            == Some("always");
        let directives_always = opts
            .and_then(|v| v.get("directiveExpressions"))
            .and_then(|v| v.as_str())
            == Some("always");
        let tags = opts.and_then(|v| v.get("tags"));
        let tags_opening_always = tags
            .and_then(|v| v.get("openingBrace"))
            .and_then(|v| v.as_str())
            == Some("always");
        let closing = tags
            .and_then(|v| v.get("closingBrace"))
            .and_then(|v| v.as_str())
            .unwrap_or("never")
            .to_string();
        let template_tag_spans = ctx.ast.html.template_tag_spans.clone();
        let template_tag_span_keys: HashSet<(u32, u32)> = template_tag_spans
            .iter()
            .map(|tag| (tag.span.start, tag.span.end))
            .collect();

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for (attr, meta) in el.attributes.iter().zip(&el.attribute_meta) {
                    match attr {
                        Attribute::Spread { span } => check_mustache(
                            ctx,
                            *span,
                            attributes_always,
                            if attributes_always { "always" } else { "never" },
                            true,
                        ),
                        Attribute::NormalAttribute { .. } => {
                            for span in mustache_spans(meta) {
                                check_mustache(
                                    ctx,
                                    span,
                                    attributes_always,
                                    if attributes_always { "always" } else { "never" },
                                    true,
                                );
                            }
                        }
                        Attribute::Directive { .. } => {
                            for span in mustache_spans(meta) {
                                check_mustache(
                                    ctx,
                                    span,
                                    directives_always,
                                    if directives_always { "always" } else { "never" },
                                    true,
                                );
                            }
                        }
                    }
                }
            }

            match node {
                TemplateNode::MustacheTag(tag) => {
                    if template_tag_span_keys.contains(&(tag.span.start, tag.span.end)) {
                        return;
                    }
                    check_mustache(
                        ctx,
                        tag.span,
                        text_always,
                        if text_always { "always" } else { "never" },
                        true,
                    );
                }
                TemplateNode::RawMustacheTag(tag) => {
                    check_mustache(ctx, tag.span, tags_opening_always, &closing, true);
                }
                TemplateNode::DebugTag(tag) => {
                    check_mustache(ctx, tag.span, tags_opening_always, &closing, true);
                }
                TemplateNode::RenderTag(tag) => {
                    check_mustache(ctx, tag.span, tags_opening_always, &closing, true);
                }
                _ => {}
            }
        });

        for tag in template_tag_spans {
            let closing_mode = if tag.check_closing {
                closing.as_str()
            } else {
                "ignore"
            };
            check_mustache(
                ctx,
                tag.span,
                tags_opening_always,
                closing_mode,
                tag.has_expression,
            );
        }
    }
}

fn mustache_spans(meta: &AttributeMeta<'_>) -> Vec<Span> {
    let mut spans = Vec::new();
    if let Some(span) = meta.mustache_span {
        spans.push(span);
    }
    for part in &meta.parts {
        if let Some(span) = part.mustache_span {
            if !spans.contains(&span) {
                spans.push(span);
            }
        }
    }
    spans
}

fn check_mustache(
    ctx: &mut LintContext<'_>,
    span: Span,
    opening_always: bool,
    closing_mode: &str,
    has_expr: bool,
) {
    if span.end <= span.start + 1 || span.end as usize > ctx.source.len() {
        return;
    }
    let open = span.start as usize;
    let close = span.end as usize - 1;
    if ctx.source.as_bytes().get(open) != Some(&b'{')
        || ctx.source.as_bytes().get(close) != Some(&b'}')
    {
        return;
    }

    let Some(first_start) = first_non_whitespace(ctx.source, open + 1, close) else {
        return;
    };
    let Some(last_end) = last_non_whitespace_end(ctx.source, open + 1, close) else {
        return;
    };

    if opening_always {
        if first_start == open + 1 {
            let insert = Span::new((open + 1) as u32, (open + 1) as u32);
            ctx.diagnostic_with_fix(
                "Expected 1 space after '{', but not found.",
                Span::new(open as u32, (open + 1) as u32),
                Fix {
                    span: insert,
                    replacement: " ".to_string(),
                },
            );
        }
    } else if first_start > open + 1 {
        let fix_span = Span::new((open + 1) as u32, first_start as u32);
        ctx.diagnostic_with_fix(
            "Expected no space after '{', but found.",
            Span::new(open as u32, first_start as u32),
            Fix {
                span: fix_span,
                replacement: String::new(),
            },
        );
    }

    if closing_mode == "ignore" {
        return;
    }

    let require_closing =
        closing_mode == "always" || (closing_mode == "always-after-expression" && has_expr);
    if require_closing {
        if last_end == close {
            let insert = Span::new(close as u32, close as u32);
            ctx.diagnostic_with_fix(
                "Expected 1 space before '}', but not found.",
                Span::new(close as u32, (close + 1) as u32),
                Fix {
                    span: insert,
                    replacement: " ".to_string(),
                },
            );
        }
    } else if last_end < close {
        let fix_span = Span::new(last_end as u32, close as u32);
        ctx.diagnostic_with_fix(
            "Expected no space before '}', but found.",
            Span::new(last_end as u32, (close + 1) as u32),
            Fix {
                span: fix_span,
                replacement: String::new(),
            },
        );
    }
}

fn first_non_whitespace(source: &str, start: usize, end: usize) -> Option<usize> {
    source[start..end]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| start + idx)
}

fn last_non_whitespace_end(source: &str, start: usize, end: usize) -> Option<usize> {
    source[start..end]
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, ch)| start + idx + ch.len_utf8())
}
