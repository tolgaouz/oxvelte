//! `svelte/no-svelte-internal` — disallow importing from svelte/internal.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

pub struct NoSvelteInternal;

impl Rule for NoSvelteInternal {
    fn name(&self) -> &'static str {
        "svelte/no-svelte-internal"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for script in [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten() {
            let tag_start = script.span.start as usize;
            let source = ctx.source;
            let content_offset = source[tag_start..].find('>').map(|p| tag_start + p + 1).unwrap_or(tag_start);
            let content = &script.content;

            // Check for `svelte/internal` as a module specifier in import/export
            // statements. Must appear inside quotes and be the actual package name
            // (not e.g. `@melt-ui/svelte/internal`).
            for target in ["'svelte/internal'", "\"svelte/internal\"",
                           "'svelte/internal/", "\"svelte/internal/"] {
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(target) {
                    let abs_in_content = search_from + pos;
                    let source_pos = content_offset + abs_in_content;
                    // Report at the `svelte/internal` part (skip quote)
                    let start = source_pos + 1; // skip opening quote
                    let svelte_internal_len = if target.ends_with('/') {
                        // Find closing quote for `svelte/internal/...`
                        let rest = &content[abs_in_content + target.len()..];
                        let q = target.as_bytes()[0];
                        let end = rest.find(q as char).unwrap_or(rest.len());
                        target.len() - 1 + end // -1 for opening quote
                    } else {
                        "svelte/internal".len()
                    };
                    ctx.diagnostic(
                        "Using svelte/internal is prohibited. This will be removed in Svelte 6.",
                        oxc::span::Span::new(start as u32, (start + svelte_internal_len) as u32),
                    );
                    search_from = abs_in_content + target.len();
                }
            }
        }
    }
}
