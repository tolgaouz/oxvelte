//! `svelte/require-stores-init` — require store variables to be initialized.

use crate::linter::{parse_imports, LintContext, Rule};

pub struct RequireStoresInit;

impl Rule for RequireStoresInit {
    fn name(&self) -> &'static str {
        "svelte/require-stores-init"
    }

    fn applies_to_scripts(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let imports = parse_imports(content);
        let factories: Vec<(String, &str)> = imports.iter()
            .filter(|(_, imp, m)| m == "svelte/store" && matches!(imp.as_str(), "writable" | "readable" | "derived"))
            .map(|(local, imp, _)| (local.clone(), imp.as_str())).collect();
        if factories.is_empty() { return; }

        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);

        for (local_name, factory) in &factories {
            let pattern = format!("{}(", local_name);
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find(&pattern) {
                let abs = search_from + pos;
                if abs > 0 && { let p = content.as_bytes()[abs - 1]; p.is_ascii_alphanumeric() || p == b'_' } {
                    search_from = abs + pattern.len(); continue;
                }
                let call_start = abs + pattern.len();
                let mut depth = 1i32;
                let mut arg_end = call_start;
                for (i, ch) in content[call_start..].char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => { depth -= 1; if depth == 0 { arg_end = call_start + i; break; } }
                        _ => {}
                    }
                }
                let args = content[call_start..arg_end].trim();
                if args.starts_with("...") { search_from = abs + pattern.len(); continue; }

                let should_report = if *factory == "derived" {
                    args.is_empty() || {
                        let mut commas = 0usize;
                        let mut d = 0i32;
                        for ch in args.chars() {
                            match ch {
                                '(' | '[' | '{' => d += 1,
                                ')' | ']' | '}' if d > 0 => d -= 1,
                                ',' if d == 0 => commas += 1,
                                _ => {}
                            }
                        }
                        commas < 2
                    }
                } else { args.is_empty() };

                if should_report {
                    let source_pos = base + gt + 1 + abs;
                    ctx.diagnostic("Always set a default value for svelte stores.",
                        oxc::span::Span::new(source_pos as u32, (base + gt + 1 + arg_end + 1) as u32));
                }
                search_from = abs + pattern.len();
            }
        }
    }
}
