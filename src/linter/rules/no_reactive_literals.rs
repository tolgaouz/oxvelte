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
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);

        let mut search_from = 0;
        while let Some(pos) = content[search_from..].find("$:") {
            let abs = search_from + pos;
            let after = content[abs + 2..].trim_start();
            if !after.starts_with('{') {
                if let Some(eq) = after.find('=') {
                    let rhs = after[eq + 1..].trim_start();
                    let is_kw = |kw: &str| rhs == kw || rhs.starts_with(&format!("{};", kw)) || rhs.starts_with(&format!("{}\n", kw));
                    let is_literal = rhs.starts_with('"') || rhs.starts_with('\'')
                        || (rhs.starts_with('`') && !rhs.contains("${"))
                        || is_kw("true") || is_kw("false") || is_kw("null") || is_kw("undefined")
                        || is_numeric_literal(rhs) || rhs.starts_with("[]") || rhs.starts_with("{}");
                    if is_literal && !after[..eq].contains('(') {
                        let sp = base + gt + 1 + abs;
                        ctx.diagnostic("Do not assign literal values inside reactive statements unless absolutely necessary.",
                            oxc::span::Span::new(sp as u32, (sp + 2) as u32));
                    }
                }
            }
            search_from = abs + 2;
        }
    }
}

fn is_numeric_literal(rhs: &str) -> bool {
    if !rhs.as_bytes().first().map_or(false, |&c| c.is_ascii_digit() || c == b'-') { return false; }
    let end = rhs.find(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-' && c != '+').unwrap_or(rhs.len());
    if end == rhs.len() { return true; }
    match rhs.as_bytes()[end] {
        b';' | b'\n' | b'\r' => true,
        b' ' | b'\t' => { let r = rhs[end..].trim_start(); r.is_empty() || r.starts_with(';') || r.starts_with('\n') }
        _ => false,
    }
}
