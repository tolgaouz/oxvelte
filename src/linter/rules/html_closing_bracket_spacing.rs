//! `svelte/html-closing-bracket-spacing` — require or disallow a space before
//! the closing bracket of HTML elements.
//! 🔧 Fixable
//!
//! Vendor reference:
//! `vendor/eslint-plugin-svelte/.../src/rules/html-closing-bracket-spacing.ts`.
//! Vendor subscribes to `SvelteStartTag` / `SvelteEndTag` (each with its own
//! range ending at the closing bracket) and reads `src.getText(node)` to
//! inspect the tag text in isolation.
//!
//! Our `Element<'a>` now carries `start_tag_end: u32` and
//! `end_tag_span: Option<Span>` so this rule can do the same: slice the
//! start-tag source (`el.span.start..=start_tag_end`) or the end-tag source
//! (`end_tag_span`) directly — no brace / quote tokenization, no
//! `rfind("</name")` against the whole element text.

use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct HtmlClosingBracketSpacing;

impl Rule for HtmlClosingBracketSpacing {
    fn name(&self) -> &'static str {
        "svelte/html-closing-bracket-spacing"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref().and_then(|v| v.as_array()).and_then(|arr| arr.first());
        let mode = |key: &str, default: &str| {
            opts.and_then(|o| o.get(key)).and_then(|v| v.as_str()).unwrap_or(default).to_string()
        };
        let start_mode = mode("startTag", "never");
        let end_mode = mode("endTag", "never");
        let sc_mode = mode("selfClosingTag", "always");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };

            // Start tag (or self-closing tag) — from `<` through `>`.
            let start_tag_bracket = el.start_tag_end;
            let start_tag_src_end = (start_tag_bracket + 1) as usize;
            if start_tag_src_end <= ctx.source.len() {
                let start_tag_src = &ctx.source[el.span.start as usize..start_tag_src_end];
                if el.self_closing {
                    if sc_mode != "ignore" {
                        check(&sc_mode, start_tag_src, start_tag_bracket, 2, ctx);
                    }
                } else if start_mode != "ignore" {
                    check(&start_mode, start_tag_src, start_tag_bracket, 1, ctx);
                }
            }

            // End tag — scoped to its own span; no rescanning the whole element.
            if end_mode != "ignore" {
                if let Some(end_span) = el.end_tag_span {
                    let end_src = &ctx.source[end_span.start as usize..end_span.end as usize];
                    let bracket = end_span.end - 1;
                    check(&end_mode, end_src, bracket, 1, ctx);
                }
            }
        });
    }
}

/// Inspect the trailing whitespace before a tag's closing bracket and
/// report/fix per `mode`. `tag_src` is the tag's own source slice ending
/// at `>` (or `/>`). `bracket_pos` is the absolute byte offset of the
/// `>` in `ctx.source`. `bracket_width` is 1 for `>`, 2 for `/>`.
///
/// Mirrors vendor's `/(\s*)\/?>$/` regex — we just count trailing space/tab
/// characters before the bracket. A newline inside the whitespace run
/// short-circuits the check (same as vendor's `containsNewline`).
fn check(mode: &str, tag_src: &str, bracket_pos: u32, bracket_width: u32, ctx: &mut LintContext) {
    let close_len = bracket_width as usize;
    if tag_src.len() < close_len { return; }
    let body = &tag_src[..tag_src.len() - close_len];
    let trimmed = body.trim_end_matches([' ', '\t', '\n', '\r']);
    let spaces = &body[trimmed.len()..];
    if spaces.contains('\n') { return; }
    let space_start = bracket_pos - spaces.len() as u32;

    match (mode, spaces.is_empty()) {
        ("always", true) => {
            ctx.diagnostic_with_fix(
                "Expected space before '>', but not found.",
                Span::new(bracket_pos, bracket_pos + bracket_width),
                Fix { span: Span::new(bracket_pos, bracket_pos), replacement: " ".to_string() },
            );
        }
        ("never", false) => {
            ctx.diagnostic_with_fix(
                "Expected no space before '>', but found.",
                Span::new(space_start, bracket_pos + bracket_width),
                Fix { span: Span::new(space_start, bracket_pos), replacement: String::new() },
            );
        }
        _ => {}
    }
}
