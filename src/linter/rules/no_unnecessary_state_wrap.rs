//! `svelte/no-unnecessary-state-wrap` — disallow wrapping values that are already reactive with `$state`.
//! ⭐ Recommended 💡
//!
//! Svelte's reactive classes (SvelteSet, SvelteMap, etc.) are already reactive
//! and don't need `$state()` wrapping.

use crate::linter::{parse_imports, LintContext, Rule};

const REACTIVE_CLASSES: &[&str] = &[
    "SvelteSet", "SvelteMap", "SvelteURL", "SvelteURLSearchParams",
    "SvelteDate", "MediaQuery",
];

pub struct NoUnnecessaryStateWrap;

impl Rule for NoUnnecessaryStateWrap {
    fn name(&self) -> &'static str {
        "svelte/no-unnecessary-state-wrap"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let tag_start = script.span.start as usize;
        let source = ctx.source;

        let imports = parse_imports(content);
        let mut names: Vec<(String, String)> = REACTIVE_CLASSES.iter()
            .map(|s| (s.to_string(), s.to_string())).collect();

        if let Some(additional) = ctx.config.options.as_ref()
            .and_then(|o| o.as_array()).and_then(|arr| arr.first())
            .and_then(|o| o.get("additionalReactiveClasses")).and_then(|v| v.as_array()) {
            for cls in additional.iter().filter_map(|c| c.as_str()) {
                names.push((cls.to_string(), cls.to_string()));
            }
        }
        for (local, imported, module) in &imports {
            if (module.starts_with("svelte/") || module == "svelte")
                && REACTIVE_CLASSES.contains(&imported.as_str()) && local != imported {
                names.push((local.clone(), imported.clone()));
            }
        }

        let allow_reassign = ctx.config.options.as_ref()
            .and_then(|o| o.as_array()).and_then(|arr| arr.first())
            .and_then(|o| o.get("allowReassign")).and_then(|v| v.as_bool()).unwrap_or(false);

        let mut search_from = 0;
        while let Some(pos) = content[search_from..].find("$state(") {
            let abs_pos = search_from + pos;
            let trimmed = content[abs_pos + 7..].trim_start();
            if trimmed.starts_with("new ") {
                let after_new = trimmed[4..].trim_start();
                if let Some((_, original)) = names.iter().find(|(l, _)| after_new.starts_with(l.as_str())) {
                    let before = content[..abs_pos].trim_end();
                    let (uses_const, uses_let) = if before.ends_with('=') {
                        let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = before[line_start..].trim_start();
                        (line.starts_with("const ") || line.starts_with("export const "),
                         line.starts_with("let ") || line.starts_with("export let "))
                    } else { (false, false) };
                    let var_reassigned = uses_let && {
                        let var_name = before[..before.len()-1].trim_end().split_whitespace().last().unwrap_or("");
                        !var_name.is_empty() && {
                            let pat = format!("{} =", var_name);
                            content.match_indices(&pat).any(|(p, _)| {
                                let ls = content[..p].rfind('\n').map(|x| x + 1).unwrap_or(0);
                                let l = content[ls..].trim_start();
                                !l.starts_with("let ") && !l.starts_with("const ")
                            }) || source.contains(&format!("bind:{}", var_name))
                               || source.contains(&format!("{} =", var_name))
                        }
                    };
                    let should_flag = uses_const || (uses_let && (!allow_reassign || !var_reassigned));
                    if should_flag {
                        if let Some(gt) = source[tag_start..script.span.end as usize].find('>') {
                            let source_pos = tag_start + gt + 1 + abs_pos;
                            ctx.diagnostic(format!("{} is already reactive, $state wrapping is unnecessary.", original),
                                oxc::span::Span::new(source_pos as u32, (source_pos + 7) as u32));
                        }
                    }
                }
            }
            search_from = abs_pos + 7;
        }
    }
}
