//! `svelte/no-export-load-in-svelte-module-in-kit-pages` — disallow exporting load in SvelteKit page module scripts.
//! ⭐ Recommended (SvelteKit)

use crate::linter::{LintContext, Rule};

pub struct NoExportLoadInSvelteModuleInKitPages;

impl Rule for NoExportLoadInSvelteModuleInKitPages {
    fn name(&self) -> &'static str {
        "svelte/no-export-load-in-svelte-module-in-kit-pages"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(fp) = &ctx.file_path {
            let fname = fp.rsplit('/').next().unwrap_or(fp);
            let is_page = ["+page.svelte", "+layout.svelte", "+error.svelte"].iter()
                .any(|s| fname == *s || fname.ends_with(s));
            if !is_page { return; }
            if let Some(routes_dir) = ctx.config.settings.as_ref()
                .and_then(|s| s.get("svelte")).and_then(|s| s.get("kit"))
                .and_then(|s| s.get("files")).and_then(|s| s.get("routes")).and_then(|s| s.as_str()) {
                if !fp.contains(routes_dir) { return; }
            }
        }
        let Some(module) = &ctx.ast.module else { return };
        if !module.content.contains("export") || !module.content.contains("load") { return; }
        for pat in &["export function load", "export const load", "export let load"] {
            if let Some(pos) = module.content.find(pat) {
                let end = pos + pat.len();
                if end < module.content.len() && { let n = module.content.as_bytes()[end]; n.is_ascii_alphanumeric() || n == b'_' } { continue; }
                let base = module.span.start as usize;
                let gt = ctx.source[base..module.span.end as usize].find('>').unwrap_or(0);
                let sp = (base + gt + 1 + end - 4) as u32;
                ctx.diagnostic("Do not export `load` from a component's module script. Use `+page.ts` or `+layout.ts` instead.",
                    oxc::span::Span::new(sp, sp + 4));
            }
        }
    }
}
