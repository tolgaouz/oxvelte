//! `svelte/html-closing-bracket-spacing` — require or disallow a space before
//! the closing bracket of HTML elements.
//! 🔧 Fixable
//!
//! Vendor reference:
//! `vendor/eslint-plugin-svelte/.../src/rules/html-closing-bracket-spacing.ts`.
//! Vendor subscribes to `'SvelteStartTag, SvelteEndTag'` (one visitor, both
//! node types) and, for each hit, runs `/(\s*)\/?>$/` on `src.getText(node)`.
//! `match[0].length` is `whitespace + bracket`, so the fix/diagnostic range
//! anchors at `node.range[1] - match[0].length`.
//!
//! Our `Element<'a>` carries `start_tag_end: u32` and
//! `end_tag_span: Option<Span>` (parser pre-records both, `#[serde(skip)]`
//! so snapshots are byte-identical). We slice the tag's own source out of
//! `ctx.source` and run the vendor-equivalent trim check.

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

            // Start tag (or self-closing). `start_tag_end` is the byte offset of
            // the `>` character; the tag's source therefore ends one past it.
            let start_src_end = (el.start_tag_end + 1) as usize;
            if start_src_end <= ctx.source.len() {
                let tag_src = &ctx.source[el.span.start as usize..start_src_end];
                let tag_mode: &str = if el.self_closing { &sc_mode } else { &start_mode };
                check_tag(tag_mode, tag_src, el.span.start, ctx);
            }

            // End tag `</name>` — scoped to its own span.
            if let Some(end_span) = el.end_tag_span {
                let tag_src = &ctx.source[end_span.start as usize..end_span.end as usize];
                check_tag(&end_mode, tag_src, end_span.start, ctx);
            }
        });
    }
}

/// Port of vendor's `/(\s*)\/?>$/` check. `tag_src` ends at `>` or `/>`;
/// `tag_src_start` is its absolute byte offset in `ctx.source`. Anchor
/// positions (`bracket_start`, `bracket_end`, `space_start`) are computed
/// exactly as vendor computes `start = range[1] - match[0].length` /
/// `end = range[1]`: `bracket_start` is the first bracket character — `/`
/// for `/>`, `>` for `>`. Insertions/removals anchor there, matching the
/// vendor's `fixer.insertTextBeforeRange([start, end], ' ')` /
/// `fixer.removeRange([start, start + spaces.length])` exactly.
fn check_tag(mode: &str, tag_src: &str, tag_src_start: u32, ctx: &mut LintContext) {
    if mode == "ignore" { return; }
    let close_len = if tag_src.ends_with("/>") {
        2
    } else if tag_src.ends_with('>') {
        1
    } else {
        return;
    };
    let body = &tag_src[..tag_src.len() - close_len];
    let trimmed = body.trim_end_matches([' ', '\t', '\n', '\r']);
    let spaces = &body[trimmed.len()..];
    if spaces.contains('\n') { return; }

    let bracket_end = tag_src_start + tag_src.len() as u32;
    let bracket_start = bracket_end - close_len as u32;
    let space_start = bracket_start - spaces.len() as u32;

    match (mode, spaces.is_empty()) {
        ("always", true) => {
            ctx.diagnostic_with_fix(
                "Expected space before '>', but not found.",
                Span::new(bracket_start, bracket_end),
                Fix {
                    span: Span::new(bracket_start, bracket_start),
                    replacement: " ".to_string(),
                },
            );
        }
        ("never", false) => {
            ctx.diagnostic_with_fix(
                "Expected no space before '>', but found.",
                Span::new(space_start, bracket_end),
                Fix {
                    span: Span::new(space_start, bracket_start),
                    replacement: String::new(),
                },
            );
        }
        _ => {}
    }
}
