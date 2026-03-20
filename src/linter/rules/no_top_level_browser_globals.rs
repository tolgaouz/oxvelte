//! `svelte/no-top-level-browser-globals` — disallow top-level access to browser globals
//! such as `window`, `document`, or `localStorage` outside lifecycle hooks.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

/// Browser globals that should not be referenced at the top level of a Svelte
/// component script because they are unavailable during SSR.
const BROWSER_GLOBALS: &[&str] = &[
    "window",
    "document",
    "navigator",
    "localStorage",
    "sessionStorage",
    "location",
    "history",
    "alert",
    "confirm",
    "prompt",
    "fetch",
    "XMLHttpRequest",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "customElements",
    "getComputedStyle",
    "matchMedia",
    "IntersectionObserver",
    "MutationObserver",
    "ResizeObserver",
];

pub struct NoTopLevelBrowserGlobals;

impl Rule for NoTopLevelBrowserGlobals {
    fn name(&self) -> &'static str {
        "svelte/no-top-level-browser-globals"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            if script.module {
                return;
            }
            let content = &script.content;
            let base = script.span.start as usize;
            for global in BROWSER_GLOBALS {
                for (byte_offset, _) in content.match_indices(global) {
                    // Simple heuristic: check that the character before is not alphanumeric
                    // and the character after is not alphanumeric/underscore (word boundary).
                    let before_ok = byte_offset == 0
                        || !content.as_bytes()[byte_offset - 1].is_ascii_alphanumeric()
                            && content.as_bytes()[byte_offset - 1] != b'_';
                    let after_pos = byte_offset + global.len();
                    let after_ok = after_pos >= content.len()
                        || !content.as_bytes()[after_pos].is_ascii_alphanumeric()
                            && content.as_bytes()[after_pos] != b'_';
                    if before_ok && after_ok {
                        let start = (base + byte_offset) as u32;
                        let end = start + global.len() as u32;
                        ctx.diagnostic(
                            format!(
                                "Avoid referencing `{}` at the top level — it is not available during SSR. Use `onMount` or a browser check.",
                                global
                            ),
                            Span::new(start, end),
                        );
                    }
                }
            }
        }
    }
}
