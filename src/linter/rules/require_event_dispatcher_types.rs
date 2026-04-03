//! `svelte/require-event-dispatcher-types` — require type parameters for createEventDispatcher.
//! ⭐ Recommended

use crate::linter::{parse_imports, LintContext, Rule};

pub struct RequireEventDispatcherTypes;

impl Rule for RequireEventDispatcherTypes {
    fn name(&self) -> &'static str {
        "svelte/require-event-dispatcher-types"
    }

    fn is_recommended(&self) -> bool {
        // The vendor rule is gated to svelteVersions: ['3/4'].
        // createEventDispatcher is deprecated in Svelte 5, so this rule adds noise
        // in Svelte 5 projects. Disable by default (opt-in).
        false
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        if script.lang.as_deref() != Some("ts") { return; }
        let content = &script.content;
        if content.contains("$props()") { return; }
        let imports = parse_imports(content);
        let names: Vec<String> = imports.iter()
            .filter(|(_, imp, m)| (imp == "createEventDispatcher" || imp == "*") && m == "svelte")
            .map(|(l, imp, _)| if imp == "*" { format!("{}.createEventDispatcher", l) } else { l.clone() })
            .collect();
        if names.is_empty() { return; }
        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);

        for name in &names {
            let pat = format!("{}()", name);
            let mut from = 0;
            while let Some(pos) = content[from..].find(&pat) {
                let abs = from + pos;
                if !content[..abs + name.len()].ends_with('>') {
                    let sp = base + gt + 1 + abs;
                    ctx.diagnostic("Type parameters missing for the `createEventDispatcher` function call.",
                        oxc::span::Span::new(sp as u32, (sp + pat.len()) as u32));
                }
                from = abs + pat.len();
            }
        }
    }
}
