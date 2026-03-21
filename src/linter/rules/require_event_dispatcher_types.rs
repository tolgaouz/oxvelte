//! `svelte/require-event-dispatcher-types` — require type parameters for createEventDispatcher.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

pub struct RequireEventDispatcherTypes;

impl Rule for RequireEventDispatcherTypes {
    fn name(&self) -> &'static str {
        "svelte/require-event-dispatcher-types"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            // Only applies to TypeScript scripts (type parameters are a TS feature)
            if script.lang.as_deref() != Some("ts") {
                return;
            }
            // Only flag if createEventDispatcher is imported from 'svelte'
            if !script.content.contains("from 'svelte'") && !script.content.contains("from \"svelte\"") {
                return;
            }
            if script.content.contains("createEventDispatcher()") {
                let tag_start = script.span.start as usize;
                let source = ctx.source;
                let mut search_from = tag_start;
                while let Some(offset) = source[search_from..].find("createEventDispatcher()") {
                    let start = search_from + offset;
                    let end = start + "createEventDispatcher()".len();
                    ctx.diagnostic(
                        "Provide type parameters for `createEventDispatcher` to specify event types.",
                        oxc::span::Span::new(start as u32, end as u32),
                    );
                    search_from = end;
                }
            }
        }
    }
}
