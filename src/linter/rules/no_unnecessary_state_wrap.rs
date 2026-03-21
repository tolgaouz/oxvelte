//! `svelte/no-unnecessary-state-wrap` — disallow wrapping values that are already reactive with `$state`.
//! ⭐ Recommended 💡
//!
//! Svelte's reactive classes (SvelteSet, SvelteMap, etc.) are already reactive
//! and don't need `$state()` wrapping.

use crate::linter::{LintContext, Rule};

const REACTIVE_CLASSES: &[&str] = &[
    "SvelteSet", "SvelteMap", "SvelteURL", "SvelteURLSearchParams",
    "SvelteDate", "MediaQuery",
];

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

            // Look for $state(new SvelteReactiveClass(...)) patterns
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find("$state(") {
                let abs_pos = search_from + pos;
                let after = &content[abs_pos + 7..];
                let trimmed = after.trim_start();

                // Check if argument is `new SvelteReactiveClass(...)`
                if trimmed.starts_with("new ") {
                    let after_new = trimmed[4..].trim_start();
                    let is_reactive = REACTIVE_CLASSES.iter().any(|cls| {
                        after_new.starts_with(cls)
                    });
                    // Only flag if the declaration uses `const` (not reassignable)
                    // Look backwards from $state( to find the declaration keyword
                    let before = content[..abs_pos].trim_end();
                    let uses_const = before.ends_with("=") && {
                        let before_eq = before[..before.len()-1].trim_end();
                        // Go back past the variable name to find const/let
                        let words: Vec<&str> = before_eq.split_whitespace().collect();
                        words.len() >= 2 && words[words.len() - 2] == "const"
                    };
                    if is_reactive && uses_const {
                        let tag_text = &source[tag_start..script.span.end as usize];
                        if let Some(gt) = tag_text.find('>') {
                            let source_pos = tag_start + gt + 1 + abs_pos;
                            ctx.diagnostic(
                                "Unnecessary `$state()` wrapping a value that is already reactive.",
                                oxc::span::Span::new(source_pos as u32, (source_pos + 7) as u32),
                            );
                        }
                    }
                }
                search_from = abs_pos + 7;
            }
        }
    }
}
