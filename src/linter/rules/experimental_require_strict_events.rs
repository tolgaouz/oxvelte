//! `svelte/experimental-require-strict-events` — require strict event typing
//! via `createEventDispatcher<Events>()`.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct ExperimentalRequireStrictEvents;

impl Rule for ExperimentalRequireStrictEvents {
    fn name(&self) -> &'static str {
        "svelte/experimental-require-strict-events"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            // Flag `createEventDispatcher()` calls without a type parameter.
            for (offset, _) in content.match_indices("createEventDispatcher()") {
                let start = (base + offset) as u32;
                let end = start + "createEventDispatcher()".len() as u32;
                ctx.diagnostic(
                    "Provide a type parameter to `createEventDispatcher<Events>()` for strict event typing.",
                    Span::new(start, end),
                );
            }
        }
    }
}
