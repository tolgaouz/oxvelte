//! `svelte/no-inspect` — disallow use of `$inspect`.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

pub struct NoInspect;

impl Rule for NoInspect {
    fn name(&self) -> &'static str {
        "svelte/no-inspect"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for script in [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten() {
            let tag_start = script.span.start as usize;
            let source = ctx.source;
            let tag_len = script.span.end as usize - tag_start;
            let tag_text = &source[tag_start..tag_start + tag_len];
            for (offset, _) in tag_text.match_indices("$inspect") {
                let start = tag_start + offset;
                let end = start + "$inspect".len();
                ctx.diagnostic(
                    "Do not use $inspect directive",
                    oxc::span::Span::new(start as u32, end as u32),
                );
            }
        }
    }
}
