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
            if fname != "+page.svelte" && fname != "+layout.svelte"
                && !fname.ends_with("+page.svelte") && !fname.ends_with("+layout.svelte") {
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
                if module.content.contains("export function load") || module.content.contains("export const load") {
                    ctx.diagnostic(
                        "Do not export `load` from a component's module script. Use `+page.ts` or `+layout.ts` instead.",
                        module.span,
                    );
                }
            }
        }
    }
}
