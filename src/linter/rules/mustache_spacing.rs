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
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let mode_always = opts.and_then(|v| v.get("textExpressions"))
            .and_then(|v| v.as_str()) == Some("always");
        let tags = opts.and_then(|v| v.get("tags"));
        let tags_opening_always = tags.and_then(|v| v.get("openingBrace"))
            .and_then(|v| v.as_str()) == Some("always");
        let closing = tags.and_then(|v| v.get("closingBrace")).and_then(|v| v.as_str()).unwrap_or("never");
        let closing_always = closing == "always";
        let closing_after_expr = closing == "always-after-expression";

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::SnippetBlock(block) = node {
                let block_src = &ctx.source[block.span.start as usize..block.span.end as usize];
                if let Some(close_brace) = block_src.find('}') {
                    let span = Span::new(block.span.start, block.span.start + (close_brace as u32) + 1);
                    check_brace_tag(&block_src[..close_brace + 1], span, tags_opening_always, closing_always, closing_after_expr, true, false, ctx);
                }
                if let Some(close_start) = block_src.rfind("{/snippet") {
                    let closing_src = &block_src[close_start..];
                    if let Some(cb) = closing_src.find('}') {
                        let span = Span::new(block.span.start + close_start as u32, block.span.start + close_start as u32 + cb as u32 + 1);
                        check_brace_tag(&closing_src[..cb + 1], span, tags_opening_always, closing_always, closing_after_expr, false, false, ctx);
                    }
                }
            }
            if let TemplateNode::RenderTag(tag) = node {
                let src = &ctx.source[tag.span.start as usize..tag.span.end as usize];
                check_brace_tag(src, tag.span, tags_opening_always, closing_always, closing_after_expr, true, false, ctx);
            }
            if let TemplateNode::MustacheTag(tag) = node {
                let (start, end) = (tag.span.start as usize, tag.span.end as usize);
                if end <= ctx.source.len() && end > start + 2 {
                    let src = &ctx.source[start..end];
                    if src.starts_with('{') && src.ends_with('}') {
                        let inner = &src[1..src.len() - 1];
                        let trimmed = inner.trim_start();
                        let is_tag = matches!(trimmed.as_bytes().first(), Some(b'#' | b'/' | b':' | b'@'));
                        if is_tag {
                            let kw_end = trimmed.find(|c: char| c.is_whitespace()).unwrap_or(trimmed.len());
                            let has_expr = !trimmed[kw_end..].trim().is_empty();
                            let is_bare = trimmed.starts_with(':') && !has_expr;
                            check_brace_tag(src, tag.span, tags_opening_always, closing_always, closing_after_expr, has_expr, is_bare, ctx);
                        } else if mode_always {
                            if !inner.starts_with(' ') || !inner.ends_with(' ') {
                                ctx.diagnostic("Expected spaces inside mustache braces. Use `{ expr }` instead of `{expr}`.", tag.span);
                            }
                        } else if inner.starts_with(' ') || inner.ends_with(' ') {
                            ctx.diagnostic("Unexpected spaces inside mustache braces. Use `{expr}` instead of `{ expr }`.", tag.span);
                        }
                    }
                }
            }
        });
    }
}

fn check_brace_tag(src: &str, span: Span, opening_always: bool, closing_always: bool, closing_after_expr: bool, has_expr: bool, is_bare: bool, ctx: &mut LintContext<'_>) {
    if !src.starts_with('{') || !src.ends_with('}') { return; }
    let inner = &src[1..src.len() - 1];
    let require_closing = (closing_always && !is_bare) || (closing_after_expr && has_expr);
    if opening_always && !inner.starts_with(' ') {
        ctx.diagnostic("Expected 1 space after '{', but not found.", span);
    }
    if require_closing && !inner.ends_with(' ') {
        ctx.diagnostic("Expected 1 space before '}', but not found.", span);
    }
    if !opening_always && inner.starts_with(' ') {
        ctx.diagnostic("Expected no space after '{', but found.", span);
    }
    if !require_closing && !is_bare && inner.ends_with(' ') {
        ctx.diagnostic("Expected no space before '}', but found.", span);
    }
}
