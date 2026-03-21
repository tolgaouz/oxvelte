//! `svelte/no-navigation-without-resolve` — disallow SvelteKit navigation calls
//! (`goto`, `pushState`, etc.) without using `$app/paths` `resolve`.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

const NAV_FUNCTIONS: &[&str] = &["goto(", "pushState(", "replaceState("];

pub struct NoNavigationWithoutResolve;

impl Rule for NoNavigationWithoutResolve {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-resolve"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            // If the script does not import `resolveRoute` or `base`, flag navigation calls
            // that take a string literal argument (starting with `/` or `'`).
            let has_resolve = content.contains("resolveRoute") || content.contains("from '$app/paths'");

            if has_resolve {
                return;
            }

            for nav_fn in NAV_FUNCTIONS {
                for (offset, _) in content.match_indices(nav_fn) {
                    // Check character before to ensure it's a word boundary.
                    if offset > 0 {
                        let prev = content.as_bytes()[offset - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            continue;
                        }
                    }
                    let rest = &content[offset + nav_fn.len()..];
                    let trimmed = rest.trim_start();
                    // Flag if the argument is a non-empty string literal.
                    // Skip empty strings ('', "", ``)
                    let is_empty_string = trimmed.starts_with("''")
                        || trimmed.starts_with("\"\"")
                        || trimmed.starts_with("``");
                    if !is_empty_string
                        && (trimmed.starts_with('\'')
                            || trimmed.starts_with('"')
                            || trimmed.starts_with('`'))
                    {
                        let start = (base + offset) as u32;
                        let end = start + nav_fn.len() as u32;
                        ctx.diagnostic(
                            format!(
                                "Use `resolveRoute` from `$app/paths` instead of passing a raw string to `{}`.",
                                &nav_fn[..nav_fn.len() - 1]
                            ),
                            Span::new(start, end),
                        );
                    }
                }
            }
        }
    }
}
