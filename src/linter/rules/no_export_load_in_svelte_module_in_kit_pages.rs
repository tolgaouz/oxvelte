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
        // Only check files that are SvelteKit page/layout files.
        // If settings.svelte.kit.files.routes is set, the file must be under that routes dir.
        // The file must be named +page.svelte or +layout.svelte.
        if let Some(file_path) = &ctx.file_path {
            let fname = file_path.rsplit('/').next().unwrap_or(file_path);
            if fname != "+page.svelte" && fname != "+layout.svelte" && fname != "+error.svelte"
                && !fname.ends_with("+page.svelte") && !fname.ends_with("+layout.svelte") && !fname.ends_with("+error.svelte") {
                return;
            }
            // Check settings for custom routes directory
            if let Some(routes_dir) = ctx.config.settings.as_ref()
                .and_then(|s| s.get("svelte"))
                .and_then(|s| s.get("kit"))
                .and_then(|s| s.get("files"))
                .and_then(|s| s.get("routes"))
                .and_then(|s| s.as_str())
            {
                if !file_path.contains(routes_dir) {
                    return;
                }
            }
        }

        if let Some(module) = &ctx.ast.module {
            if module.content.contains("export") && module.content.contains("load") {
                // Simple heuristic: check if "export" and "load" appear together
                // Check for `export function load` or `export const load` with word boundary
                for pattern in &["export function load", "export const load", "export let load"] {
                    if let Some(pos) = module.content.find(pattern) {
                        let after_pos = pos + pattern.len();
                        // Word boundary check: next char must not be alphanumeric or _
                        if after_pos < module.content.len() {
                            let next = module.content.as_bytes()[after_pos];
                            if next.is_ascii_alphanumeric() || next == b'_' {
                                continue;
                            }
                        }
                        // Compute span at the `load` identifier
                        let load_offset = pos + pattern.len() - 4; // "load" is 4 chars
                        let base = module.span.start as usize;
                        let tag_text = &ctx.source[base..module.span.end as usize];
                        let gt = tag_text.find('>').unwrap_or(0);
                        let source_pos = (base + gt + 1 + load_offset) as u32;
                        ctx.diagnostic(
                            "Do not export `load` from a component's module script. Use `+page.ts` or `+layout.ts` instead.",
                            oxc::span::Span::new(source_pos, source_pos + 4),
                        );
                    }
                }
            }
        }
    }
}
