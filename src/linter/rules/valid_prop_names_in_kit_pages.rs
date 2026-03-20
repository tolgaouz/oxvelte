//! `svelte/valid-prop-names-in-kit-pages` — ensure exported props in SvelteKit pages
//! use valid names (`data`, `form`, `snapshot`).
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

/// Valid prop names for SvelteKit page components.
const VALID_KIT_PROPS: &[&str] = &["data", "form", "snapshot"];

pub struct ValidPropNamesInKitPages;

impl Rule for ValidPropNamesInKitPages {
    fn name(&self) -> &'static str {
        "svelte/valid-prop-names-in-kit-pages"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
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
                            "Prop `{}` is not a valid SvelteKit page prop. Expected one of: {}.",
                            prop_name,
                            VALID_KIT_PROPS.join(", ")
                        ),
                        Span::new(start, end),
                    );
                }
            }
        }
    }
}
