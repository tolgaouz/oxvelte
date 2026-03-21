//! `svelte/require-event-dispatcher-types` — require type parameters for createEventDispatcher.
//! ⭐ Recommended

use crate::linter::{parse_imports, LintContext, Rule};

pub struct RequireEventDispatcherTypes;

impl Rule for RequireEventDispatcherTypes {
    fn name(&self) -> &'static str {
        "svelte/require-event-dispatcher-types"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            // Only applies to TypeScript scripts
            if script.lang.as_deref() != Some("ts") {
                return;
            }

            let content = &script.content;
            let imports = parse_imports(content);

            // Find local names for createEventDispatcher from 'svelte'
            let dispatcher_names: Vec<String> = imports.iter()
                .filter(|(_, imported, module)| {
                    (imported == "createEventDispatcher" || imported == "*") && module == "svelte"
                })
                .map(|(local, imported, _)| {
                    if imported == "*" { format!("{}.createEventDispatcher", local) } else { local.clone() }
                })
                .collect();

            if dispatcher_names.is_empty() { return; }

            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            for name in &dispatcher_names {
                // Look for name() without type parameters (name<...>() has type params)
                let pattern = format!("{}()", name);
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(&pattern) {
                    let abs = search_from + pos;
                    // Check it's not name<...>() — look for < before ()
                    let before = &content[..abs + name.len()];
                    if !before.ends_with('>') {
                        let source_pos = base + gt + 1 + abs;
                        ctx.diagnostic(
                            "Provide type parameters for `createEventDispatcher` to specify event types.",
                            oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                        );
                    }
                    search_from = abs + pattern.len();
                }
            }
        }
    }
}
