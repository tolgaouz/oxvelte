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
        let opts = ctx.config.options.as_ref().and_then(|v| v.as_array()).and_then(|arr| arr.first());
        let get = |key, default| opts.and_then(|o| o.get(key)).and_then(|v| v.as_str()).unwrap_or(default).to_string();
        let (start_opt, end_opt, sc_opt) = (get("startTag", "never"), get("endTag", "never"), get("selfClosingTag", "always"));

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            let el_start = el.span.start as usize;
            let tag_text = &ctx.source[el_start..el.span.end as usize];

            let check = |mode: &str, before: &str, bracket_rel: usize, width: u32, ctx: &mut LintContext| {
                let spaces = { let t = before.trim_end_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r'); &before[t.len()..] };
                if spaces.contains('\n') { return; }
                let ss = (el_start + bracket_rel - spaces.len()) as u32;
                let br = (el_start + bracket_rel) as u32;
                if mode == "always" && spaces.is_empty() {
                    ctx.diagnostic_with_fix("Expected space before '>', but not found.", Span::new(br, br + width),
                        Fix { span: Span::new(br, br), replacement: " ".to_string() });
                } else if mode == "never" && !spaces.is_empty() {
                    ctx.diagnostic_with_fix("Expected no space before '>', but found.", Span::new(ss, br + width),
                        Fix { span: Span::new(ss, br), replacement: "".to_string() });
                }
            };

            if el.self_closing {
                if sc_opt == "ignore" { return; }
                if let Some(rel) = find_tag_bracket(tag_text, true) { check(&sc_opt, &tag_text[..rel], rel, 2, ctx); }
            } else {
                if start_opt != "ignore" {
                    if let Some(rel) = find_tag_bracket(tag_text, false) { check(&start_opt, &tag_text[..rel], rel, 1, ctx); }
                }
                if end_opt != "ignore" {
                    if let Some((_, br)) = find_end_tag_bracket(tag_text, &el.name) { check(&end_opt, &tag_text[..br], br, 1, ctx); }
                }
            }
        });
    }
}

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

fn find_end_tag_bracket(tag_text: &str, name: &str) -> Option<(usize, usize)> {
    let pat = format!("</{}", name);
    let start = tag_text.rfind(&pat)?;
    let br = start + pat.len() + tag_text[start + pat.len()..].find('>')?;
    Some((start, br))
}
