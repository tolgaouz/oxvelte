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
                    module == "svelte/store" && (imported == "writable" || imported == "readable")
                })
                .map(|(local, imported, _)| (local.clone(), imported.as_str()))
                .collect();

            if store_factories.is_empty() { return; }

            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            for (local_name, _factory) in &store_factories {
                // Look for factory() calls without arguments
                let pattern = format!("{}()", local_name);
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
                    let source_pos = base + gt + 1 + abs;
                    ctx.diagnostic(
                        format!("Store `{}` should be initialized with a value.", local_name),
                        oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                    );
                    search_from = abs + pattern.len();
                }
            }
        }
    }
}
