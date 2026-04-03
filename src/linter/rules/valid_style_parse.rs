//! `svelte/valid-style-parse` — report style parsing errors in `<style>` blocks.

use crate::linter::{LintContext, Rule};
use crate::parser::css::CssParser;

pub struct ValidStyleParse;

impl Rule for ValidStyleParse {
    fn name(&self) -> &'static str {
        "svelte/valid-style-parse"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(style) = &ctx.ast.css else { return };
        if style.content.trim().is_empty() { return; }
        if let Some(lang) = &style.lang {
            if !["css", "scss", "less", "postcss", "stylus", "sass"].contains(&lang.as_str()) {
                ctx.diagnostic(format!("Found unsupported style element language \"{}\"", lang), style.span);
                return;
            }
        }
        let tag_text = &ctx.source[style.span.start as usize..style.span.end as usize];
        let cs = tag_text.find('>').map(|p| style.span.start + p as u32 + 1).unwrap_or(style.span.start);
        let mut parser = CssParser::new(&style.content, cs);
        let _ = parser.parse_rules();
        let err_pos = if !parser.error_positions.is_empty() { Some(parser.error_positions[0] as u32) }
            else if !style.content[parser.pos..].trim().is_empty() { Some(parser.pos as u32) }
            else { None };
        if let Some(ep) = err_pos {
            ctx.diagnostic("CSS parsing error in <style> block.", oxc::span::Span::new(cs + ep, style.span.end));
        }
    }
}
