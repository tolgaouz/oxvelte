//! `svelte/no-add-event-listener` — disallow `addEventListener` in Svelte components.
//! 💡

use crate::linter::{LintContext, Rule};

pub struct NoAddEventListener;

impl Rule for NoAddEventListener {
    fn name(&self) -> &'static str { "svelte/no-add-event-listener" }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);
        for method in &["addEventListener(", ".addEventListener("] {
            let mut from = 0;
            while let Some(pos) = script.content[from..].find(method) {
                let abs = from + pos;
                let sp = base + gt + 1 + abs;
                ctx.diagnostic("Do not use `addEventListener`. Use the `on` function from `svelte/events` instead.",
                    oxc::span::Span::new(sp as u32, (sp + method.len()) as u32));
                from = abs + method.len();
            }
        }
    }
}
