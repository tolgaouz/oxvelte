//! `svelte/no-unnecessary-state-wrap` — disallow wrapping values that are already reactive with `$state`.
//! ⭐ Recommended 💡
//!
//! Svelte's reactive classes (SvelteSet, SvelteMap, etc.) are already reactive
//! and don't need `$state()` wrapping.

use crate::linter::{parse_imports, LintContext, Rule};

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

            // Build a mapping of local names -> original reactive class names
            let imports = parse_imports(content);
            let mut reactive_local_names: Vec<String> = REACTIVE_CLASSES.iter().map(|s| s.to_string()).collect();
            for (local, imported, module) in &imports {
                if module.starts_with("svelte/") || module == "svelte" {
                    if REACTIVE_CLASSES.contains(&imported.as_str()) && local != imported {
                        // Aliased import: import { SvelteSet as CustomSet }
                        reactive_local_names.push(local.clone());
                    }
                }
            }

            // Look for $state(new ReactiveClass(...)) patterns
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find("$state(") {
                let abs_pos = search_from + pos;
                let after = &content[abs_pos + 7..];
                let trimmed = after.trim_start();

                // Check if argument is `new ReactiveClass(...)`
                if trimmed.starts_with("new ") {
                    let after_new = trimmed[4..].trim_start();
                    let is_reactive = reactive_local_names.iter().any(|cls| {
                        after_new.starts_with(cls.as_str())
                    });
                    // Only flag const declarations (let/var might be reassigned)
                    let before = content[..abs_pos].trim_end();
                    let uses_const = before.ends_with('=') && {
                        let before_eq = before[..before.len()-1].trim_end();
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
