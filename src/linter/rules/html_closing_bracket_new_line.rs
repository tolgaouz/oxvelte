//! `svelte/html-closing-bracket-new-line` — require or disallow a newline before
//! the closing bracket of elements.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct HtmlClosingBracketNewLine;

impl Rule for HtmlClosingBracketNewLine {
    fn name(&self) -> &'static str {
        "svelte/html-closing-bracket-new-line"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Parse config options
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        let singleline_opt = opts
            .and_then(|o| o.get("singleline"))
            .and_then(|v| v.as_str())
            .unwrap_or("never");
        let multiline_opt = opts
            .and_then(|o| o.get("multiline"))
            .and_then(|v| v.as_str())
            .unwrap_or("always");

        let singleline_expect_newline = singleline_opt == "always";
        let multiline_expect_newline = multiline_opt == "always";

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let (span, attrs_count, source) = match node {
                TemplateNode::Element(el) => (el.span, el.attributes.len(), ctx.source),
                _ => return,
            };

            let tag_text = &source[span.start as usize..span.end as usize];

            // Find the opening tag end (first > or />)
            let mut depth = 0;
            let mut open_bracket_end = None;
            let bytes = tag_text.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' | b'\'' => {
                        let q = bytes[i];
                        i += 1;
                        while i < bytes.len() && bytes[i] != q { i += 1; }
                    }
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    b'>' if depth == 0 => {
                        open_bracket_end = Some(i);
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }

            let bracket_pos = match open_bracket_end {
                Some(p) => p,
                None => return,
            };

            // Determine if self-closing
            let is_self_closing = bracket_pos > 0 && bytes[bracket_pos - 1] == b'/';

            // The bracket is `>` or `/>`. Find the actual bracket start
            let bracket_start = if is_self_closing { bracket_pos - 1 } else { bracket_pos };

            // Find the content before the bracket (between tag name/last attr and bracket)
            let before_bracket = &tag_text[..bracket_start];

            // Count line breaks between the last non-whitespace content and the bracket
            let last_content_pos = before_bracket.rfind(|c: char| !c.is_whitespace()).unwrap_or(0);
            let between = &before_bracket[last_content_pos + 1..];
            let line_breaks = between.chars().filter(|&c| c == '\n').count();

            // Determine if the element is multiline (has attributes and they span multiple lines)
            let first_line_end = tag_text.find('\n').unwrap_or(tag_text.len());
            let is_multiline = attrs_count > 0 && first_line_end < bracket_start;

            // Also check closing tags like </div\n\n\n  >
            // Find closing tag if present
            let close_tag_start = if !is_self_closing {
                // Find the closing tag </name>
                let tag_name_end = tag_text[1..].find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != ':' && c != '.')
                    .map(|p| p + 1).unwrap_or(1);
                let name = &tag_text[1..tag_name_end];
                let close_pattern = format!("</{}", name);
                tag_text.rfind(&close_pattern)
            } else { None };

            if let Some(close_start) = close_tag_start {
                // Check closing tag for line breaks: </name ... >
                let close_text = &tag_text[close_start..];
                let close_name_end = close_text.find('>').unwrap_or(close_text.len());
                let close_before_bracket = &close_text[..close_name_end];
                let close_line_breaks = close_before_bracket.chars().filter(|&c| c == '\n').count();
                if close_line_breaks > 0 {
                    let abs_pos = span.start + close_start as u32;
                    ctx.diagnostic(
                        format!("Expected no line breaks before closing bracket, but {} line break{} found.",
                            close_line_breaks, if close_line_breaks != 1 { "s" } else { "" }),
                        oxc::span::Span::new(abs_pos, abs_pos + close_name_end as u32 + 1),
                    );
                }
            }

            if is_multiline {
                if multiline_expect_newline {
                    // Multiline: expect exactly 1 line break
                    if line_breaks != 1 {
                        let abs_pos = span.start + bracket_start as u32;
                        if line_breaks == 0 {
                            ctx.diagnostic(
                                "Expected 1 line break before closing bracket, but no line breaks found.",
                                oxc::span::Span::new(abs_pos, abs_pos + if is_self_closing { 2 } else { 1 }),
                            );
                        } else {
                            ctx.diagnostic(
                                format!("Expected 1 line break before closing bracket, but {} line breaks found.", line_breaks),
                                oxc::span::Span::new(abs_pos, abs_pos + if is_self_closing { 2 } else { 1 }),
                            );
                        }
                    }
                } else {
                    // Multiline "never": expect 0 line breaks
                    if line_breaks > 0 {
                        let abs_pos = span.start + bracket_start as u32;
                        ctx.diagnostic(
                            format!("Expected no line breaks before closing bracket, but {} line break{} found.",
                                line_breaks, if line_breaks != 1 { "s" } else { "" }),
                            oxc::span::Span::new(abs_pos, abs_pos + if is_self_closing { 2 } else { 1 }),
                        );
                    }
                }
            } else {
                if singleline_expect_newline {
                    // Singleline "always": expect a line break
                    if line_breaks == 0 {
                        let abs_pos = span.start + bracket_start as u32;
                        ctx.diagnostic(
                            "Expected 1 line break before closing bracket, but no line breaks found.",
                            oxc::span::Span::new(abs_pos, abs_pos + if is_self_closing { 2 } else { 1 }),
                        );
                    }
                } else {
                    // Singleline "never": expect 0 line breaks
                    if line_breaks > 0 {
                        let abs_pos = span.start + bracket_start as u32;
                        ctx.diagnostic(
                            format!("Expected no line breaks before closing bracket, but {} line break{} found.",
                                line_breaks, if line_breaks != 1 { "s" } else { "" }),
                            oxc::span::Span::new(abs_pos, abs_pos + if is_self_closing { 2 } else { 1 }),
                        );
                    }
                }
            }
        });
    }
}
