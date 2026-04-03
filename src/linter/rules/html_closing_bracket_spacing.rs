//! `svelte/html-closing-bracket-spacing` — require or disallow a space before
//! the closing bracket of HTML elements.
//! 🔧 Fixable

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
        // Parse config options
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        let start_tag_opt = opts
            .and_then(|o| o.get("startTag"))
            .and_then(|v| v.as_str())
            .unwrap_or("never")
            .to_string();
        let end_tag_opt = opts
            .and_then(|o| o.get("endTag"))
            .and_then(|v| v.as_str())
            .unwrap_or("never")
            .to_string();
        let self_closing_opt = opts
            .and_then(|o| o.get("selfClosingTag"))
            .and_then(|v| v.as_str())
            .unwrap_or("always")
            .to_string();

        let source = ctx.source;

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };

            let el_start = el.span.start as usize;
            let el_end = el.span.end as usize;
            let tag_text = &source[el_start..el_end];

            // --- Check start tag (or self-closing tag) ---
            if el.self_closing {
                if self_closing_opt == "ignore" { return; }
                // Find `/>` in the opening tag
                if let Some(rel) = find_tag_bracket(tag_text, true) {
                    // `rel` points to the `/` of `/>` within tag_text
                    let before_slash = &tag_text[..rel];
                    // Get trailing whitespace before `/>`
                    let spaces = trailing_spaces(before_slash);
                    // Multiline exemption: skip if spacing contains a newline
                    if spaces.contains('\n') { return; }

                    let spaces_start = rel - spaces.len();
                    let abs_spaces_start = (el_start + spaces_start) as u32;
                    let abs_close = (el_start + rel) as u32;

                    if self_closing_opt == "always" && spaces.is_empty() {
                        // Need a space before `/>`
                        ctx.diagnostic_with_fix(
                            "Expected space before '>', but not found.",
                            Span::new(abs_close, abs_close + 2),
                            Fix {
                                span: Span::new(abs_close, abs_close),
                                replacement: " ".to_string(),
                            },
                        );
                    } else if self_closing_opt == "never" && !spaces.is_empty() {
                        // Should not have space before `/>`
                        ctx.diagnostic_with_fix(
                            "Expected no space before '>', but found.",
                            Span::new(abs_spaces_start, abs_close + 2),
                            Fix {
                                span: Span::new(abs_spaces_start, abs_close),
                                replacement: "".to_string(),
                            },
                        );
                    }
                }
            } else {
                // Non-self-closing: check start tag and end tag separately

                // --- Start tag ---
                if start_tag_opt != "ignore" {
                    if let Some(rel) = find_tag_bracket(tag_text, false) {
                        // rel points to `>` of the opening tag
                        let before_bracket = &tag_text[..rel];
                        let spaces = trailing_spaces(before_bracket);
                        if !spaces.contains('\n') {
                            let spaces_start = rel - spaces.len();
                            let abs_spaces_start = (el_start + spaces_start) as u32;
                            let abs_bracket = (el_start + rel) as u32;

                            if start_tag_opt == "always" && spaces.is_empty() {
                                ctx.diagnostic_with_fix(
                                    "Expected space before '>', but not found.",
                                    Span::new(abs_bracket, abs_bracket + 1),
                                    Fix {
                                        span: Span::new(abs_bracket, abs_bracket),
                                        replacement: " ".to_string(),
                                    },
                                );
                            } else if start_tag_opt == "never" && !spaces.is_empty() {
                                ctx.diagnostic_with_fix(
                                    "Expected no space before '>', but found.",
                                    Span::new(abs_spaces_start, abs_bracket + 1),
                                    Fix {
                                        span: Span::new(abs_spaces_start, abs_bracket),
                                        replacement: "".to_string(),
                                    },
                                );
                            }
                        }
                    }
                }

                // --- End tag ---
                if end_tag_opt != "ignore" {
                    // Find the closing tag </name ...> in the element source
                    if let Some((end_tag_rel, bracket_rel)) = find_end_tag_bracket(tag_text, &el.name) {
                        let _ = end_tag_rel;
                        let before_bracket = &tag_text[..bracket_rel];
                        let spaces = trailing_spaces(before_bracket);
                        if !spaces.contains('\n') {
                            let spaces_start = bracket_rel - spaces.len();
                            let abs_spaces_start = (el_start + spaces_start) as u32;
                            let abs_bracket = (el_start + bracket_rel) as u32;

                            if end_tag_opt == "always" && spaces.is_empty() {
                                ctx.diagnostic_with_fix(
                                    "Expected space before '>', but not found.",
                                    Span::new(abs_bracket, abs_bracket + 1),
                                    Fix {
                                        span: Span::new(abs_bracket, abs_bracket),
                                        replacement: " ".to_string(),
                                    },
                                );
                            } else if end_tag_opt == "never" && !spaces.is_empty() {
                                ctx.diagnostic_with_fix(
                                    "Expected no space before '>', but found.",
                                    Span::new(abs_spaces_start, abs_bracket + 1),
                                    Fix {
                                        span: Span::new(abs_spaces_start, abs_bracket),
                                        replacement: "".to_string(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}

/// Return trailing whitespace (spaces/tabs, not newlines accounted for separately) at the end of `s`.
/// Actually returns ALL trailing whitespace chars including newlines so caller can check for newlines.
fn trailing_spaces(s: &str) -> &str {
    let trimmed_end = s.trim_end_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r');
    &s[trimmed_end.len()..]
}

/// Scan a tag for a closing bracket, skipping strings and expressions.
/// If `self_closing`, looks for `/>` and returns position of `/`.
/// Otherwise looks for `>` and returns its position.
fn find_tag_bracket(tag_text: &str, self_closing: bool) -> Option<usize> {
    let bytes = tag_text.as_bytes();
    let mut i = 1; // skip `<`
    let mut depth = 0i32;
    let limit = if self_closing { bytes.len().saturating_sub(1) } else { bytes.len() };
    while i < limit {
        match bytes[i] {
            b'"' | b'\'' => {
                let q = bytes[i]; i += 1;
                while i < bytes.len() && bytes[i] != q { i += 1; }
            }
            b'{' => depth += 1,
            b'}' => { if depth > 0 { depth -= 1; } }
            b'/' if self_closing && depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                return Some(i);
            }
            b'>' if !self_closing && depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Find the closing tag `</name>` in `tag_text` and return
/// `(end_tag_start_rel, bracket_rel)` where `bracket_rel` is the offset of `>`.
fn find_end_tag_bracket(tag_text: &str, name: &str) -> Option<(usize, usize)> {
    let pattern = format!("</{}", name);
    // Find the last occurrence (rfind) to get the actual closing tag
    let end_tag_rel = tag_text.rfind(&pattern)?;
    let rest = &tag_text[end_tag_rel + pattern.len()..];
    // Find the `>` within the rest
    let bracket_in_rest = rest.find('>')?;
    let bracket_rel = end_tag_rel + pattern.len() + bracket_in_rest;
    Some((end_tag_rel, bracket_rel))
}
