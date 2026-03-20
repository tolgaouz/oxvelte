//! `svelte/no-unnecessary-state-wrap` — disallow wrapping non-reactive values with `$state`.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

pub struct NoUnnecessaryStateWrap;

impl Rule for NoUnnecessaryStateWrap {
    fn name(&self) -> &'static str {
        "svelte/no-unnecessary-state-wrap"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let tag_start = script.span.start as usize;
            let source = ctx.source;

            // Look for $state(<literal>) patterns where the value is a simple literal
            // that doesn't need reactive wrapping.
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find("$state(") {
                let abs_pos = search_from + pos;
                let after = &content[abs_pos + 7..];

                // Check if the argument is a simple literal (number, string, boolean, null)
                let trimmed = after.trim_start();
                let is_non_reactive = trimmed.starts_with('"')
                    || trimmed.starts_with('\'')
                    || trimmed.starts_with("true")
                    || trimmed.starts_with("false")
                    || trimmed.starts_with("null")
                    || trimmed.starts_with("undefined")
                    || trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false);

                if is_non_reactive {
                    let tag_text = &source[tag_start..script.span.end as usize];
                    if let Some(gt) = tag_text.find('>') {
                        let source_pos = tag_start + gt + 1 + abs_pos;
                        ctx.diagnostic(
                            "Unnecessary `$state()` wrapping a non-reactive literal value. Consider using a plain `let` binding instead.",
                            oxc::span::Span::new(source_pos as u32, (source_pos + 7) as u32),
                        );
                    }
                }
                search_from = abs_pos + 7;
            }
        }
    }
}
