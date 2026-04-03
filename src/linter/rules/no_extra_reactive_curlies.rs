//! `svelte/no-extra-reactive-curlies` — disallow unnecessary curly braces in reactive statements.
//! 💡
//!
//! Detects `$: { single_statement; }` patterns where the braces are unnecessary.

use crate::linter::{LintContext, Rule};

pub struct NoExtraReactiveCurlies;

impl Rule for NoExtraReactiveCurlies {
    fn name(&self) -> &'static str {
        "svelte/no-extra-reactive-curlies"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);

        let mut from = 0;
        while let Some(pos) = content[from..].find("$:") {
            let abs = from + pos;
            let after = content[abs + 2..].trim_start();
            if after.starts_with('{') {
                let mut depth = 0i32;
                let close = after.char_indices().find_map(|(i, ch)| {
                    match ch { '{' => depth += 1, '}' => { depth -= 1; if depth == 0 { return Some(i); } } _ => {} }
                    None
                });
                if let Some(close) = close {
                    if after[1..close].trim().matches(';').count() <= 1 {
                        let ws = content[abs + 2..].len() - after.len();
                        let sp = base + gt + 1 + abs + 2 + ws;
                        ctx.diagnostic("Do not wrap reactive statements in curly braces unless necessary.",
                            oxc::span::Span::new(sp as u32, (sp + 1) as u32));
                    }
                }
            }
            from = abs + 2;
        }
    }
}
