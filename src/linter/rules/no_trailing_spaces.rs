//! `svelte/no-trailing-spaces` — disallow trailing whitespace at the end of lines.
//! 🔧 Fixable (Extension Rule)

use crate::linter::{LintContext, Rule, Fix};
use oxc::span::Span;

pub struct NoTrailingSpaces;

impl Rule for NoTrailingSpaces {
    fn name(&self) -> &'static str {
        "svelte/no-trailing-spaces"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for (line_num, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim_end();
            if trimmed.len() < line.len() {
                let line_start: usize = ctx.source.lines().take(line_num).map(|l| l.len() + 1).sum();
                let trailing_start = line_start + trimmed.len();
                let trailing_end = line_start + line.len();
                ctx.diagnostic_with_fix(
                    "Trailing spaces not allowed.",
                    Span::new(trailing_start as u32, trailing_end as u32),
                    Fix {
                        span: Span::new(trailing_start as u32, trailing_end as u32),
                        replacement: String::new(),
                    },
                );
            }
        }
    }
}
