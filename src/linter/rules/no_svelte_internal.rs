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
            if script.content.contains("svelte/internal") {
                let tag_start = script.span.start as usize;
                let source = ctx.source;
                let mut search_from = tag_start;
                while let Some(offset) = source[search_from..].find("svelte/internal") {
                    let start = search_from + offset;
                    let end = start + "svelte/internal".len();
                    ctx.diagnostic(
                        "Do not import from `svelte/internal`. It is not a public API.",
                        oxc::span::Span::new(start as u32, end as u32),
                    );
                    search_from = end;
                }
            }
        }
    }
}
