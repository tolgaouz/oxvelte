//! `svelte/no-reactive-literals` — disallow assignments of literal values in reactive statements.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

pub struct NoReactiveLiterals;

impl Rule for NoReactiveLiterals {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-literals"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            // Look for $: x = <literal>
            let content = &script.content;
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find("$:") {
                let abs_pos = search_from + pos;
                let after = content[abs_pos + 2..].trim_start();
                // Check if it's a simple assignment to a literal
                if let Some(eq_pos) = after.find('=') {
                    let rhs = after[eq_pos + 1..].trim_start();
                    // Only flag simple literals (not template literals with ${},
                    // arrays, or objects which may contain reactive values)
                    let is_literal = rhs.starts_with('"') || rhs.starts_with('\'')
                        || (rhs.starts_with('`') && !rhs.contains("${"))
                        || rhs.starts_with("true;") || rhs.starts_with("true\n") || rhs == "true"
                        || rhs.starts_with("false;") || rhs.starts_with("false\n") || rhs == "false"
                        || rhs.starts_with("null;") || rhs.starts_with("null\n") || rhs == "null"
                        || rhs.starts_with("undefined;") || rhs.starts_with("undefined\n") || rhs == "undefined"
                        || rhs.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false);

                    if is_literal && !after[..eq_pos].contains('(') {
                        let tag_start = script.span.start as usize;
                        let source = ctx.source;
                        let tag_text = &source[tag_start..script.span.end as usize];
                        if let Some(gt) = tag_text.find('>') {
                            let source_pos = tag_start + gt + 1 + abs_pos;
                            ctx.diagnostic(
                                "Don't use a reactive statement to assign a literal value. Use `let` or `const` instead.",
                                oxc::span::Span::new(source_pos as u32, (source_pos + 2) as u32),
                            );
                        }
                    }
                }
                search_from = abs_pos + 2;
            }
        }
    }
}
