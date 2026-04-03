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
            let base = script.span.start as usize;
            let co = ctx.source[base..].find('>').map(|p| base + p + 1).unwrap_or(base);
            for target in ["'svelte/internal'", "\"svelte/internal\"", "'svelte/internal/", "\"svelte/internal/"] {
                let mut from = 0;
                while let Some(pos) = script.content[from..].find(target) {
                    let abs = from + pos;
                    let start = co + abs + 1;
                    let len = if target.ends_with('/') {
                        let q = target.as_bytes()[0];
                        target.len() - 1 + script.content[abs + target.len()..].find(q as char).unwrap_or(0)
                    } else { 15 };
                    ctx.diagnostic("Using svelte/internal is prohibited. This will be removed in Svelte 6.",
                        oxc::span::Span::new(start as u32, (start + len) as u32));
                    from = abs + target.len();
                }
            }
        }
    }
}
