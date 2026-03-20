//! `svelte/no-add-event-listener` — disallow `addEventListener` in Svelte components.
//! 💡

use crate::linter::{LintContext, Rule};

const EVENT_LISTENER_METHODS: &[&str] = &[
    "addEventListener(",
    ".addEventListener(",
];

pub struct NoAddEventListener;

impl Rule for NoAddEventListener {
    fn name(&self) -> &'static str {
        "svelte/no-add-event-listener"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let tag_start = script.span.start as usize;
            let source = ctx.source;

            for method in EVENT_LISTENER_METHODS {
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(method) {
                    let abs_pos = search_from + pos;
                    let tag_text = &source[tag_start..script.span.end as usize];
                    if let Some(gt) = tag_text.find('>') {
                        let source_pos = tag_start + gt + 1 + abs_pos;
                        ctx.diagnostic(
                            "Avoid using `addEventListener`. Use Svelte's `on:event` directive or `$effect` with cleanup instead.",
                            oxc::span::Span::new(source_pos as u32, (source_pos + method.len()) as u32),
                        );
                    }
                    search_from = abs_pos + method.len();
                }
            }
        }
    }
}
