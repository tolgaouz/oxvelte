//! `svelte/valid-style-parse` — report style parsing errors in `<style>` blocks.

use crate::linter::{LintContext, Rule};
use crate::parser::css::CssParser;

pub struct ValidStyleParse;

impl Rule for ValidStyleParse {
    fn name(&self) -> &'static str {
        "svelte/valid-style-parse"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(style) = &ctx.ast.css {
            let content = &style.content;
            if content.trim().is_empty() {
                return;
            }

            // Flag unknown languages
            if let Some(lang) = &style.lang {
                let known = ["css", "scss", "less", "postcss", "stylus", "sass"];
                if !known.contains(&lang.as_str()) {
                    ctx.diagnostic(
                        format!("Unknown style language: '{}'.", lang),
                        style.span,
                    );
                    return;
                }
            }

            // Try to parse the CSS (even for SCSS/LESS since basic syntax is similar)
            let source = ctx.source;
            let tag_text = &source[style.span.start as usize..style.span.end as usize];
            let content_start = tag_text.find('>').map(|p| style.span.start + p as u32 + 1).unwrap_or(style.span.start);

            let mut parser = CssParser::new(content, content_start);
            let _result = parser.parse_rules();

            // Check for parsing errors (positions where the parser couldn't advance)
            if !parser.error_positions.is_empty() {
                let first_err = parser.error_positions[0];
                ctx.diagnostic(
                    "CSS parsing error in <style> block.",
                    oxc::span::Span::new(content_start + first_err as u32, style.span.end),
                );
                return;
            }

            // Check if there are unparsed characters (parsing stopped early = syntax error)
            let remaining = content[parser.pos..].trim();
            if !remaining.is_empty() {
                ctx.diagnostic(
                    "CSS parsing error in <style> block.",
                    oxc::span::Span::new(content_start + parser.pos as u32, style.span.end),
                );
            }
        }
    }
}
