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

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            // Look for readable/writable/derived with async callbacks
            for factory in &["readable(", "writable(", "derived("] {
                for (offset, _) in content.match_indices(factory) {
                    // Check preceding character for word boundary
                    if offset > 0 {
                        let prev = content.as_bytes()[offset - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            continue;
                        }
                    }

                    let rest = &content[offset + factory.len()..];

                    // Find the second argument (after first comma at depth 0)
                    let mut depth = 0i32;
                    let mut found_comma = false;
                    let mut callback_start = 0;
                    for (i, ch) in rest.char_indices() {
                        match ch {
                            '(' | '[' | '{' => depth += 1,
                            ')' | ']' | '}' => {
                                depth -= 1;
                                if depth < 0 { break; }
                            }
                            ',' if depth == 0 && !found_comma => {
                                found_comma = true;
                                callback_start = i + 1;
                            }
                            _ => {}
                        }
                    }

                    if !found_comma { continue; }

                    let callback = rest[callback_start..].trim_start();
                    if callback.starts_with("async ") || callback.starts_with("async(") {
                        let start = (base + offset) as u32;
                        let end = start + factory.len() as u32;
                        ctx.diagnostic(
                            "Do not use async functions in store callbacks. The return value of the callback should be an unsubscribe function, not a Promise.",
                            Span::new(start, end),
                        );
                    }
                }
            }
        }
    }
}
