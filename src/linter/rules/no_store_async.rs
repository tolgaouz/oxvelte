//! `svelte/no-store-async` — disallow async functions in store callbacks.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct NoStoreAsync;

impl Rule for NoStoreAsync {
    fn name(&self) -> &'static str {
        "svelte/no-store-async"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn applies_to_scripts(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;

        for factory in &["readable(", "writable(", "derived("] {
            for (offset, _) in content.match_indices(factory) {
                if offset > 0 && { let p = content.as_bytes()[offset - 1]; p.is_ascii_alphanumeric() || p == b'_' } { continue; }
                let rest = &content[offset + factory.len()..];
                let (mut depth, mut found, mut cb_start) = (0i32, false, 0);
                for (i, ch) in rest.char_indices() {
                    match ch {
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' | '}' => { depth -= 1; if depth < 0 { break; } }
                        ',' if depth == 0 && !found => { found = true; cb_start = i + 1; }
                        _ => {}
                    }
                }
                if !found { continue; }
                let cb = rest[cb_start..].trim_start();
                if cb.starts_with("async ") || cb.starts_with("async(") {
                    let start = (base + offset) as u32;
                    ctx.diagnostic("Do not pass async functions to svelte stores.", Span::new(start, start + factory.len() as u32));
                }
            }
        }
    }
}
