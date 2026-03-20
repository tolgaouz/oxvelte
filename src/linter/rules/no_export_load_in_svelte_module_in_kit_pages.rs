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
