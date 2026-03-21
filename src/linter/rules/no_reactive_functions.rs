//! `svelte/no-reactive-functions` — disallow assigning functions to reactive declarations.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

pub struct NoReactiveFunctions;

impl Rule for NoReactiveFunctions {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-functions"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            // Find $: x = () => or $: x = function
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find("$:") {
                let abs = search_from + pos;
                let after = content[abs + 2..].trim_start();
                // Look for assignment
                if let Some(eq_pos) = after.find('=') {
                    let var_part = &after[..eq_pos].trim();
                    // Make sure it looks like a variable name (not ==, !=, etc.)
                    if !var_part.is_empty()
                        && var_part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && after.as_bytes().get(eq_pos + 1) != Some(&b'=')
                    {
                        let rhs = after[eq_pos + 1..].trim_start();
                        if rhs.starts_with("() =>")
                            || rhs.starts_with("(")
                            && rhs.contains("=>")
                            || rhs.starts_with("function")
                        {
                            let source_pos = base + gt + 1 + abs;
                            ctx.diagnostic(
                                "Don't define functions in reactive statements. Use `const` instead.",
                                oxc::span::Span::new(source_pos as u32, (source_pos + 2) as u32),
                            );
                        }
                    }
                }
                search_from = abs + 2;
            }
        }
    }
}
