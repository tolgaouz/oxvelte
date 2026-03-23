//! `svelte/no-ignored-unsubscribe` — disallow ignoring store subscribe return value.

use crate::linter::{LintContext, Rule};

pub struct NoIgnoredUnsubscribe;

impl Rule for NoIgnoredUnsubscribe {
    fn name(&self) -> &'static str {
        "svelte/no-ignored-unsubscribe"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            for (i, _) in content.match_indices(".subscribe(") {
                // Find the start of this expression (go back past the variable name)
                let before = &content[..i];
                let before_trimmed = before.trim_end();
                // Check what's before the variable name
                let _var_end = before_trimmed.len();
                let var_start = before_trimmed.rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.')
                    .map(|p| p + 1).unwrap_or(0);
                let stmt_before = &before_trimmed[..var_start].trim_end();

                // If the statement before the variable is a statement boundary
                // (;, {, }, newline, start) and NOT an assignment (=, let, const, etc.)
                let is_standalone = stmt_before.is_empty()
                    || stmt_before.ends_with(';')
                    || stmt_before.ends_with('{')
                    || stmt_before.ends_with('}')
                    || stmt_before.ends_with('\n');

                // Make sure it's not assigned: no = before
                let is_assigned = stmt_before.ends_with('=')
                    || stmt_before.ends_with("const")
                    || stmt_before.ends_with("let")
                    || stmt_before.ends_with("var");

                if is_standalone && !is_assigned {
                    let source_pos = base + gt + 1 + var_start;
                    let source_end = base + gt + 1 + i + ".subscribe(".len();
                    ctx.diagnostic(
                        "Store subscribe() return value (unsubscribe function) is being ignored.",
                        oxc::span::Span::new(source_pos as u32, source_end as u32),
                    );
                }
            }
        }
    }
}
