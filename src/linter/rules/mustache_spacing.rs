//! `svelte/mustache-spacing` — enforce consistent spacing inside mustache braces `{ }`.
//! 🔧 Fixable
//!
//! Default: "never" — no spaces inside mustache braces: `{expr}`.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct MustacheSpacing;

impl Rule for MustacheSpacing {
    fn name(&self) -> &'static str {
        "svelte/mustache-spacing"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Config: { "textExpressions": "always"|"never", "attributesAndProps": ..., "directiveExpressions": ..., "tags": { "openingBrace": ..., "closingBrace": ... } }
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let mode_always = opts
            .and_then(|v| v.get("textExpressions"))
            .and_then(|v| v.as_str())
            .map(|s| s == "always")
            .unwrap_or(false);
        let tags_opening_always = opts
            .and_then(|v| v.get("tags"))
            .and_then(|v| v.get("openingBrace"))
            .and_then(|v| v.as_str())
            .map(|s| s == "always")
            .unwrap_or(false);
        let tags_closing_mode = opts
            .and_then(|v| v.get("tags"))
            .and_then(|v| v.get("closingBrace"))
            .and_then(|v| v.as_str())
            .unwrap_or("never")
            .to_string();
        // "always" = always require space, "always-after-expression" = only for tags with expressions, "never" = no space
        let tags_closing_always = tags_closing_mode == "always";

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            // Check SnippetBlock opening and closing tags
            if let TemplateNode::SnippetBlock(block) = node {
                let block_src = &ctx.source[block.span.start as usize..block.span.end as usize];
                // Find the opening tag: {#snippet...}
                if let Some(close_brace) = block_src.find('}') {
                    let opening = &block_src[..close_brace + 1];
                    let opening_span = Span::new(block.span.start, block.span.start + (close_brace as u32) + 1);
                    check_brace_tag(opening, opening_span, tags_opening_always, &tags_closing_mode, true, ctx);
                }
                // Find the closing tag: {/snippet}
                if let Some(close_start) = block_src.rfind("{/snippet") {
                    let closing = &block_src[close_start..];
                    if let Some(cb) = closing.find('}') {
                        let closing_tag = &closing[..cb + 1];
                        let closing_span = Span::new(
                            block.span.start + close_start as u32,
                            block.span.start + close_start as u32 + cb as u32 + 1,
                        );
                        check_brace_tag(closing_tag, closing_span, tags_opening_always, &tags_closing_mode, false, ctx);
                    }
                }
            }
            // Check RenderTag
            if let TemplateNode::RenderTag(tag) = node {
                let src = &ctx.source[tag.span.start as usize..tag.span.end as usize];
                check_brace_tag(src, tag.span, tags_opening_always, &tags_closing_mode, true, ctx);
            }

            if let TemplateNode::MustacheTag(tag) = node {
                let start = tag.span.start as usize;
                let end = tag.span.end as usize;
                if end <= ctx.source.len() && end > start + 2 {
                    let src = &ctx.source[start..end];
                    if src.starts_with('{') && src.ends_with('}') {
                        let inner = &src[1..src.len() - 1];
                        let trimmed_inner = inner.trim_start();
                        // Check if this is a block/special tag: {#...}, {/...}, {:...}, {@...}
                        let is_tag = trimmed_inner.starts_with('#') || trimmed_inner.starts_with('/')
                            || trimmed_inner.starts_with(':') || trimmed_inner.starts_with('@');

                        if is_tag {
                            // For block/special tags, use the tags config
                            let keyword_end = trimmed_inner.find(|c: char| c.is_whitespace())
                                .unwrap_or(trimmed_inner.len());
                            let after_keyword = trimmed_inner[keyword_end..].trim();
                            let has_expression = !after_keyword.is_empty();
                            let is_bare_continuation = trimmed_inner.starts_with(':') && !has_expression;

                            let require_closing = (tags_closing_mode == "always" && !is_bare_continuation)
                                || (tags_closing_mode == "always-after-expression" && has_expression);

                            if tags_opening_always && !inner.starts_with(' ') {
                                ctx.diagnostic(
                                    "Expected 1 space after '{', but not found.",
                                    Span::new(tag.span.start, tag.span.end),
                                );
                            }
                            if require_closing && !inner.ends_with(' ') {
                                ctx.diagnostic(
                                    "Expected 1 space before '}', but not found.",
                                    Span::new(tag.span.start, tag.span.end),
                                );
                            }
                            if !tags_opening_always && inner.starts_with(' ') {
                                ctx.diagnostic(
                                    "Unexpected space after '{'.",
                                    Span::new(tag.span.start, tag.span.end),
                                );
                            }
                            if !require_closing && !is_bare_continuation && inner.ends_with(' ') {
                                ctx.diagnostic(
                                    "Unexpected space before '}'.",
                                    Span::new(tag.span.start, tag.span.end),
                                );
                            }
                        } else if mode_always {
                            // "always" mode: spaces must be present inside braces
                            if !inner.starts_with(' ') || !inner.ends_with(' ') {
                                ctx.diagnostic(
                                    "Expected spaces inside mustache braces. Use `{ expr }` instead of `{expr}`.",
                                    Span::new(tag.span.start, tag.span.end),
                                );
                            }
                        } else {
                            // "never" mode (default): no spaces should be present
                            if inner.starts_with(' ') || inner.ends_with(' ') {
                                ctx.diagnostic(
                                    "Unexpected spaces inside mustache braces. Use `{expr}` instead of `{ expr }`.",
                                    Span::new(tag.span.start, tag.span.end),
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}

fn check_brace_tag(src: &str, span: Span, opening_always: bool, closing_mode: &str, has_expression: bool, ctx: &mut LintContext<'_>) {
    if src.starts_with('{') && src.ends_with('}') {
        let inner = &src[1..src.len() - 1];
        let require_closing_space = closing_mode == "always"
            || (closing_mode == "always-after-expression" && has_expression);
        if opening_always && !inner.starts_with(' ') {
            ctx.diagnostic("Expected 1 space after '{', but not found.", span);
        }
        if require_closing_space && !inner.ends_with(' ') {
            ctx.diagnostic("Expected 1 space before '}', but not found.", span);
        }
        if !opening_always && inner.starts_with(' ') {
            ctx.diagnostic("Unexpected space after '{'.", span);
        }
        if !require_closing_space && inner.ends_with(' ') {
            ctx.diagnostic("Unexpected space before '}'.", span);
        }
    }
}
