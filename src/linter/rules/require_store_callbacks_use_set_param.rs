//! `svelte/require-store-callbacks-use-set-param` — require that store callbacks
//! use the `set` parameter provided by the callback.
//! 💡 Has suggestion

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct RequireStoreCallbacksUseSetParam;

impl Rule for RequireStoreCallbacksUseSetParam {
    fn name(&self) -> &'static str {
        "svelte/require-store-callbacks-use-set-param"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            for factory in &["readable(", "writable("] {
                for (offset, _) in content.match_indices(factory) {
                    // Check preceding character to avoid matching inside other identifiers
                    if offset > 0 {
                        let prev = content.as_bytes()[offset - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            continue;
                        }
                    }

                    let rest = &content[offset..];

                    // Find the callback: either an arrow function or function keyword
                    // Look for the second argument (after first comma at depth 0)
                    let mut depth = 0;
                    let mut found_comma = false;
                    let mut callback_start = 0;
                    for (i, ch) in rest.char_indices() {
                        match ch {
                            '(' | '[' | '{' => depth += 1,
                            ')' | ']' | '}' => {
                                depth -= 1;
                                if depth == 0 { break; }
                            }
                            ',' if depth == 1 && !found_comma => {
                                found_comma = true;
                                callback_start = i + 1;
                            }
                            _ => {}
                        }
                    }

                    if !found_comma { continue; }

                    let callback = rest[callback_start..].trim_start();

                    // Check if callback uses `set` parameter
                    let has_set = if callback.starts_with("function") {
                        // function (set) { ... } or function () { ... }
                        if let Some(paren_start) = callback.find('(') {
                            if let Some(paren_end) = callback[paren_start..].find(')') {
                                let params = &callback[paren_start+1..paren_start+paren_end];
                                params.split(',').any(|p| p.trim() == "set")
                            } else { false }
                        } else { false }
                    } else if let Some(arrow_pos) = callback.find("=>") {
                        // Arrow function: (set) => or set =>
                        let before_arrow = callback[..arrow_pos].trim();
                        if before_arrow.starts_with('(') && before_arrow.ends_with(')') {
                            let params = &before_arrow[1..before_arrow.len()-1];
                            params.split(',').any(|p| p.trim() == "set")
                        } else {
                            before_arrow == "set"
                        }
                    } else {
                        // Not a recognizable callback
                        continue;
                    };

                    if !has_set {
                        let start = (base + offset) as u32;
                        let end = start + factory.len() as u32;
                        ctx.diagnostic(
                            "Store callbacks must use `set` param.",
                            Span::new(start, end),
                        );
                    }
                }
            }
        }
    }
}
