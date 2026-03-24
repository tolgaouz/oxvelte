//! `svelte/require-stores-init` — require store variables to be initialized.

use crate::linter::{parse_imports, LintContext, Rule};

pub struct RequireStoresInit;

impl Rule for RequireStoresInit {
    fn name(&self) -> &'static str {
        "svelte/require-stores-init"
    }

    fn applies_to_scripts(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let imports = parse_imports(content);

            // Find local names for writable/readable from svelte/store
            let store_factories: Vec<(String, &str)> = imports.iter()
                .filter(|(_, imported, module)| {
                    module == "svelte/store" && (imported == "writable" || imported == "readable" || imported == "derived")
                })
                .map(|(local, imported, _)| (local.clone(), imported.as_str()))
                .collect();

            if store_factories.is_empty() { return; }

            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            for (local_name, factory) in &store_factories {
                // For writable/readable: look for factory() calls without arguments
                // For derived: need at least 3 args (store, fn, initial_value)
                let pattern = format!("{}(", local_name);
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(&pattern) {
                    let abs = search_from + pos;
                    // Word boundary check
                    if abs > 0 {
                        let prev = content.as_bytes()[abs - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            search_from = abs + pattern.len();
                            continue;
                        }
                    }
                    // Extract the arguments by finding matching close paren
                    let call_start = abs + pattern.len();
                    let mut depth = 1;
                    let mut arg_end = call_start;
                    for (i, ch) in content[call_start..].char_indices() {
                        match ch {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    arg_end = call_start + i;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let args_text = content[call_start..arg_end].trim();

                    // Skip spread arguments — can't statically determine arg count
                    if args_text.starts_with("...") {
                        search_from = abs + pattern.len();
                        continue;
                    }

                    let should_report = if *factory == "derived" {
                        // derived needs at least 3 arguments
                        if args_text.is_empty() {
                            true
                        } else {
                            // Count top-level commas
                            let mut comma_count = 0;
                            let mut d = 0;
                            for ch in args_text.chars() {
                                match ch {
                                    '(' | '[' | '{' => d += 1,
                                    ')' | ']' | '}' => d -= 1,
                                    ',' if d == 0 => comma_count += 1,
                                    _ => {}
                                }
                            }
                            comma_count < 2 // less than 3 args
                        }
                    } else {
                        // writable/readable: need at least 1 argument
                        args_text.is_empty()
                    };

                    if should_report {
                        let source_pos = base + gt + 1 + abs;
                        let end_pos = base + gt + 1 + arg_end + 1; // include closing paren
                        ctx.diagnostic(
                            "Always set a default value for svelte stores.".to_string(),
                            oxc::span::Span::new(source_pos as u32, end_pos as u32),
                        );
                    }
                    search_from = abs + pattern.len();
                }
            }
        }
    }
}
