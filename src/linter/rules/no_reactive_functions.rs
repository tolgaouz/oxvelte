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
                            || is_arrow_function(rhs)
                            || rhs.starts_with("function")
                        {
                            let source_pos = base + gt + 1 + abs;
                            ctx.diagnostic(
                                "Do not create functions inside reactive statements unless absolutely necessary.",
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

/// Check if `rhs` is an arrow function: `(params) => ...`
/// Must NOT match expressions like `(expr).method((x) => ...)` where
/// the `=>` is inside a nested callback, not the top-level expression.
fn is_arrow_function(rhs: &str) -> bool {
    if !rhs.starts_with('(') { return false; }
    // Find the matching closing paren for the opening one
    let bytes = rhs.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    // Check if followed by `=>`
                    let rest = rhs[i + 1..].trim_start();
                    return rest.starts_with("=>");
                }
            }
            b'\'' | b'"' | b'`' => {
                // Skip string literals
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' { i += 1; }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}
