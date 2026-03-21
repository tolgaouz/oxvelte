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
        // Look for `readable(` or `writable(` calls in the instance script
        // where the callback does not reference `set`.
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            for factory in &["readable(", "writable("] {
                for (offset, _) in content.match_indices(factory) {
                    // Find the callback arrow or function after the opening paren.
                    // This is a heuristic: look for `=>` without `set` before the next `)`.
                    let rest = &content[offset..];
                    if let Some(arrow_pos) = rest.find("=>") {
                        let after_arrow = &rest[arrow_pos..];
                        let segment_end = after_arrow.find('}').map(|p| arrow_pos + p).unwrap_or(rest.len());
                        if segment_end <= arrow_pos { continue; }
                        let body = &rest[arrow_pos..segment_end];
                        if !body.contains("set") {
                            let start = (base + offset) as u32;
                            let end = start + factory.len() as u32;
                            ctx.diagnostic(
                                "Store callback should use the `set` parameter.",
                                Span::new(start, end),
                            );
                        }
                    }
                }
            }
        }
    }
}
