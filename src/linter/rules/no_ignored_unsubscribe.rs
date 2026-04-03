//! `svelte/no-ignored-unsubscribe` — disallow ignoring store subscribe return value.

use crate::linter::{LintContext, Rule};

pub struct NoIgnoredUnsubscribe;

impl Rule for NoIgnoredUnsubscribe {
    fn name(&self) -> &'static str {
        "svelte/no-ignored-unsubscribe"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);

        for (i, _) in content.match_indices(".subscribe(") {
            let bt = content[..i].trim_end();
            let vs = bt.rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.').map(|p| p + 1).unwrap_or(0);
            let sb = bt[..vs].trim_end();
            let standalone = sb.is_empty() || sb.ends_with(';') || sb.ends_with('{') || sb.ends_with('}') || sb.ends_with('\n');
            let assigned = sb.ends_with('=') || sb.ends_with("const") || sb.ends_with("let") || sb.ends_with("var");
            if standalone && !assigned {
                ctx.diagnostic("Store subscribe() return value (unsubscribe function) is being ignored.",
                    oxc::span::Span::new((base + gt + 1 + vs) as u32, (base + gt + 1 + i + 11) as u32));
            }
        }
    }
}
