//! `svelte/no-ignored-unsubscribe` — disallow ignoring store subscribe return value.

use crate::linter::{LintContext, Rule};

pub struct NoIgnoredUnsubscribe;

impl Rule for NoIgnoredUnsubscribe {
    fn name(&self) -> &'static str {
        "svelte/no-ignored-unsubscribe"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            // Simple heuristic: look for .subscribe() calls not assigned to a variable
            let content = &script.content;
            for (i, _) in content.match_indices(".subscribe(") {
                // Check if this is at the start of a statement (not part of an assignment)
                let before = &content[..i];
                let trimmed = before.trim_end();
                if trimmed.ends_with(';') || trimmed.ends_with('{') || trimmed.ends_with('}')
                    || trimmed.is_empty()
                {
                    let offset = script.span.start as usize;
                    let source = ctx.source;
                    let tag_text = &source[offset..script.span.end as usize];
                    if let Some(gt) = tag_text.find('>') {
                        let abs_pos = offset + gt + 1 + i;
                        ctx.diagnostic(
                            "Store subscribe() return value (unsubscribe function) is being ignored.",
                            oxc::span::Span::new(abs_pos as u32, (abs_pos + 11) as u32),
                        );
                    }
                }
            }
        }
    }
}
