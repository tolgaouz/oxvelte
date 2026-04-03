//! `svelte/valid-prop-names-in-kit-pages` — ensure exported props in SvelteKit pages
//! use valid names (`data`, `form`, `snapshot`).
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

/// Valid prop names for SvelteKit page components (Svelte 4 `export let` style).
const VALID_KIT_PROPS: &[&str] = &["data", "errors", "form", "params", "snapshot"];

/// Valid prop names for Svelte 5 `$props()` destructuring in SvelteKit page components.
/// Includes `children` because it is a valid snippet prop in Svelte 5.
const VALID_KIT_PROPS_SVELTE5: &[&str] = &["data", "errors", "form", "params", "snapshot", "children"];

pub struct ValidPropNamesInKitPages;

impl Rule for ValidPropNamesInKitPages {
    fn name(&self) -> &'static str {
        "svelte/valid-prop-names-in-kit-pages"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Only check files that are SvelteKit page/layout/error files.
        let Some(file_path) = &ctx.file_path else { return; };
        let fname = file_path.rsplit('/').next().unwrap_or(file_path);
        // Also handle Windows paths
        let fname = fname.rsplit('\\').next().unwrap_or(fname);
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

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);
            let base = script.span.start as usize + gt + 1;

            // Look for `export let <name>` or `export let { ... }` patterns.
            for (offset, _) in content.match_indices("export let ") {
                let rest = &content[offset + "export let ".len()..];

                if rest.starts_with('{') {
                    // Destructured export: `export let { baz, qux } = data;`
                    if let Some(close) = rest.find('}') {
                        let inner = &rest[1..close];
                        let inner_base = base + offset + "export let ".len() + 1;
                        check_destructured_props(inner, inner_base, VALID_KIT_PROPS, ctx);
                    }
                } else {
                    // Simple export: `export let foo`
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
                            "disallow props other than data or errors in SvelteKit page components.".to_string(),
                            Span::new(start, end),
                        );
                    }
                }
            }

            // Svelte 5: `let { foo, bar } = $props()`
            for (offset, _) in content.match_indices("$props()") {
                // Walk backwards from `$props()` to find the opening `{` of `let {`
                let before = &content[..offset];
                if let Some(brace_pos) = rfind_let_brace(before) {
                    // Extract content between `{` and matching `}`
                    let after_brace = &content[brace_pos + 1..];
                    if let Some(close) = after_brace.find('}') {
                        let inner = &after_brace[..close];
                        let inner_base = base + brace_pos + 1;
                        check_destructured_props(inner, inner_base, VALID_KIT_PROPS_SVELTE5, ctx);
                    }
                }
            }
        }
    }
}

fn rfind_let_brace(s: &str) -> Option<usize> {
    let mut pos = s.len();
    while let Some(p) = s[..pos].rfind('{') {
        if s[..p].trim_end().ends_with("let") { return Some(p); }
        pos = p;
    }
    None
}

/// Check each property name in a destructuring pattern like `baz, qux` or `data: data2, foo`.
/// Reports diagnostics for any property key that is not in `valid_props`.
fn check_destructured_props(
    inner: &str,
    inner_base: usize,
    valid_props: &[&str],
    ctx: &mut LintContext<'_>,
) {
    let mut pos = 0;
    for token in inner.split(',') {
        let token_trimmed = token.trim();
        if token_trimmed.is_empty() {
            pos += token.len() + 1;
            continue;
        }

        // Handle rest element `...rest`
        let name_part = if token_trimmed.starts_with("...") {
            &token_trimmed[3..]
        } else {
            // For `key: localVar`, the prop name is the key (left side)
            token_trimmed.split(':').next().unwrap_or(token_trimmed).trim()
        };

        // Handle default values: `foo = defaultVal` → prop name is `foo`
        let prop_name = name_part.split('=').next().unwrap_or(name_part).trim();

        if !prop_name.is_empty() && !valid_props.contains(&prop_name) {
            // Find the offset of prop_name within the token (accounting for leading whitespace)
            let leading_ws = token.len() - token.trim_start().len();
            let name_offset_in_trimmed = token_trimmed.find(prop_name).unwrap_or(0);
            let byte_offset = inner_base + pos + leading_ws + name_offset_in_trimmed;
            let start = byte_offset as u32;
            let end = (byte_offset + prop_name.len()) as u32;
            ctx.diagnostic(
                "disallow props other than data or errors in SvelteKit page components.".to_string(),
                Span::new(start, end),
            );
        }

        pos += token.len() + 1; // +1 for the comma separator
    }
}
