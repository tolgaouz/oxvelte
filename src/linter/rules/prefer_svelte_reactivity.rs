//! `svelte/prefer-svelte-reactivity` — prefer Svelte reactivity helpers over manual
//! patterns (e.g. prefer `$derived` over manually computed values).
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

/// Patterns in instance script that suggest manual reactivity instead of Svelte runes.
const MANUAL_PATTERNS: &[(&str, &str)] = &[
    ("$: ", "Prefer `$derived` or `$effect` over reactive statements (`$:`)"),
    (".subscribe(", "Prefer `$store` auto-subscription over manual `.subscribe()` calls"),
];

pub struct PreferSvelteReactivity;

impl Rule for PreferSvelteReactivity {
    fn name(&self) -> &'static str {
        "svelte/prefer-svelte-reactivity"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            for &(pattern, message) in MANUAL_PATTERNS {
                for (offset, _) in content.match_indices(pattern) {
                    let start = (base + offset) as u32;
                    let end = start + pattern.len() as u32;
                    ctx.diagnostic(message, Span::new(start, end));
                }
            }
        }
    }
}
