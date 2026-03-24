//! `svelte/valid-prop-names-in-kit-pages` — ensure exported props in SvelteKit pages
//! use valid names (`data`, `form`, `snapshot`).
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

/// Valid prop names for SvelteKit page components.
const VALID_KIT_PROPS: &[&str] = &["data", "errors", "form", "params", "snapshot"];

pub struct ValidPropNamesInKitPages;

impl Rule for ValidPropNamesInKitPages {
    fn name(&self) -> &'static str {
        "svelte/valid-prop-names-in-kit-pages"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Only check files that are SvelteKit page/layout files (+page.svelte, +layout.svelte).
        if let Some(file_path) = &ctx.file_path {
            let fname = file_path.rsplit('/').next().unwrap_or(file_path);
            if fname != "+page.svelte" && fname != "+layout.svelte" && fname != "+error.svelte" {
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

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            // Look for `export let <name>` patterns.
            for (offset, _) in content.match_indices("export let ") {
                let rest = &content[offset + "export let ".len()..];
                let var_end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                if var_end == 0 {
                    continue;
                }
                let prop_name = &rest[..var_end];
                if !VALID_KIT_PROPS.contains(&prop_name) {
                    let start = (base + offset) as u32;
                    let end = (base + offset + "export let ".len() + var_end) as u32;
                    ctx.diagnostic(
                        format!(
                            "disallow props other than data or errors in SvelteKit page components."
                        ),
                        Span::new(start, end),
                    );
                }
            }
        }
    }
}
