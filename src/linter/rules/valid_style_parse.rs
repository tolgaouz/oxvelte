//! `svelte/valid-style-parse` — report style parsing errors in `<style>` blocks.

use crate::linter::{LintContext, Rule};
use crate::parser::css::parse_css;

pub struct ValidStyleParse;

const SUPPORTED_STYLE_LANGS: &[&str] = &["css", "scss", "less", "postcss", "stylus", "sass"];

impl Rule for ValidStyleParse {
    fn name(&self) -> &'static str {
        "svelte/valid-style-parse"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(style) = &ctx.ast.css else { return };
        if style.content.trim().is_empty() {
            return;
        }
        if let Some(lang) = &style.lang {
            if !SUPPORTED_STYLE_LANGS.contains(&lang.as_str()) {
                ctx.diagnostic(
                    format!("Found unsupported style element language \"{}\"", lang),
                    style.span,
                );
                return;
            }
        }
        // Upstream reports parser-service style context failures. Until this
        // project has dedicated preprocessors for every style lang, the
        // Svelte-compatible CSS parser is the canonical syntax check here.
        let tag_text = &ctx.source[style.span.start as usize..style.span.end as usize];
        let cs = tag_text
            .find('>')
            .map(|p| style.span.start + p as u32 + 1)
            .unwrap_or(style.span.start);
        let parsed = parse_css(&style.content, cs);
        let err_pos = if let Some(error) = parsed.errors.first() {
            Some(error.position as u32)
        } else if !parsed.error_positions.is_empty() {
            Some(parsed.error_positions[0] as u32)
        } else if !style.content[parsed.position..].trim().is_empty() {
            Some(parsed.position as u32)
        } else {
            None
        };
        if let Some(ep) = err_pos {
            ctx.diagnostic(
                "CSS parsing error in <style> block.",
                oxc::span::Span::new(cs + ep, style.span.end),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linter::LintContext;
    use crate::parser;
    use oxc::allocator::Allocator;

    fn diagnostics_for(source: &str) -> Vec<String> {
        let allocator = Allocator::default();
        let parsed = parser::parse(source, &allocator);
        let mut ctx = LintContext::new(&parsed.ast, source);
        ValidStyleParse.run(&mut ctx);
        ctx.into_diagnostics()
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn accepts_common_scss_syntax_without_preprocessor() {
        let messages = diagnostics_for(
            r#"<style lang="scss">
                $brand: red;
                %button { color: $brand; }
                .button { @extend %button; }
            </style>"#,
        );

        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn reports_unknown_style_language_before_parsing() {
        let messages = diagnostics_for(r#"<style lang="wat">.x { color red; }</style>"#);
        assert_eq!(
            messages,
            vec!["Found unsupported style element language \"wat\"".to_string()]
        );
    }
}
