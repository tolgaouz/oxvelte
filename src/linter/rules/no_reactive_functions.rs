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
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);

        let mut search_from = 0;
        while let Some(pos) = content[search_from..].find("$:") {
            let abs = search_from + pos;
            let after = content[abs + 2..].trim_start();
            if let Some(eq_pos) = after.find('=') {
                let var_part = after[..eq_pos].trim();
                if !var_part.is_empty()
                    && var_part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$')
                    && after.as_bytes().get(eq_pos + 1) != Some(&b'=')
                {
                    let rhs = after[eq_pos + 1..].trim_start();
                    if rhs.starts_with("() =>") || is_arrow_function(rhs)
                        || rhs.starts_with("function ") || rhs.starts_with("function(") {
                        let sp = base + gt + 1 + abs;
                        ctx.diagnostic("Do not create functions inside reactive statements unless absolutely necessary.",
                            oxc::span::Span::new(sp as u32, (sp + 2) as u32));
                    }
                }
            }
            search_from = abs + 2;
        }
    }
}

fn is_arrow_function(rhs: &str) -> bool {
    if !rhs.starts_with('(') { return false; }
    let bytes = rhs.as_bytes();
    let (mut depth, mut i) = (0i32, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let rest = rhs[i + 1..].trim_start();
                    return rest.starts_with("=>") || (rest.starts_with(':') && rest.contains("=>"));
                }
            }
            b'\'' | b'"' | b'`' => {
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
