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
        if let Some(script) = &ctx.ast.instance {
            if script.content.contains("$inspect") {
                // Find position of $inspect in the script content
                let tag_start = script.span.start as usize;
                let source = ctx.source;
                if let Some(offset) = source[tag_start..].find("$inspect") {
                    let start = tag_start + offset;
                    let end = start + "$inspect".len();
                    ctx.diagnostic(
                        "Unexpected `$inspect`. `$inspect` should only be used during development.",
                        oxc::span::Span::new(start as u32, end as u32),
                    );
                }
            }
        }
    }
}
