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
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let tag_start = script.span.start as usize;
            let source = ctx.source;

            // Build a mapping of local names -> original reactive class names
            let imports = parse_imports(content);
            let mut reactive_local_names: Vec<String> = REACTIVE_CLASSES.iter().map(|s| s.to_string()).collect();

            // Add additional reactive classes from config
            if let Some(options) = &ctx.config.options {
                if let Some(arr) = options.as_array() {
                    for opt in arr {
                        if let Some(additional) = opt.get("additionalReactiveClasses").and_then(|v| v.as_array()) {
                            for cls in additional {
                                if let Some(s) = cls.as_str() {
                                    reactive_local_names.push(s.to_string());
                                }
                            }
                        }
                    }
                }
            }
            for (local, imported, module) in &imports {
                if module.starts_with("svelte/") || module == "svelte" {
                    if REACTIVE_CLASSES.contains(&imported.as_str()) && local != imported {
                        // Aliased import: import { SvelteSet as CustomSet }
                        reactive_local_names.push(local.clone());
                    }
                }
            }

            // Check allowReassign config
            let allow_reassign = ctx.config.options.as_ref()
                .and_then(|o| o.as_array())
                .and_then(|arr| arr.first())
                .and_then(|o| o.get("allowReassign"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Look for $state(new ReactiveClass(...)) patterns
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find("$state(") {
                let abs_pos = search_from + pos;
                let after = &content[abs_pos + 7..];
                let trimmed = after.trim_start();

                if trimmed.starts_with("new ") {
                    let after_new = trimmed[4..].trim_start();
                    let is_reactive = reactive_local_names.iter().any(|cls| {
                        after_new.starts_with(cls.as_str())
                    });
                    // Check declaration type
                    let before = content[..abs_pos].trim_end();
                    let (uses_const, uses_let) = if before.ends_with('=') {
                        let before_eq = before[..before.len()-1].trim_end();
                        let words: Vec<&str> = before_eq.split_whitespace().collect();
                        let kw = words.get(words.len().wrapping_sub(2)).copied().unwrap_or("");
                        (kw == "const", kw == "let")
                    } else { (false, false) };
                    // With allowReassign, flag let vars only if NOT reassigned
                    let var_is_reassigned = if uses_let {
                        // Extract variable name and check for reassignment
                        let before_eq = before[..before.len()-1].trim_end();
                        let var_name = before_eq.split_whitespace().last().unwrap_or("");
                        !var_name.is_empty() && {
                            let reassign = format!("{} =", var_name);
                            // Check script content for reassignment
                            let script_reassign = content.match_indices(&reassign).any(|(p, _)| {
                                let line_start = content[..p].rfind('\n').map(|x| x + 1).unwrap_or(0);
                                let line = content[line_start..].trim_start();
                                !line.starts_with("let ") && !line.starts_with("const ")
                            });
                            // Also check full source for template-level reassignment (bind:)
                            let template_reassign = source.contains(&format!("bind:{}", var_name))
                                || source.contains(&format!("{} =", var_name));
                            script_reassign || template_reassign
                        }
                    } else { false };
                    let should_flag = uses_const || (allow_reassign && uses_let && !var_is_reassigned);
                    if is_reactive && should_flag {
                        let tag_text = &source[tag_start..script.span.end as usize];
                        // Find the matching reactive class name
                        let class_name = reactive_local_names.iter()
                            .find(|cls| after_new.starts_with(cls.as_str()))
                            .cloned()
                            .unwrap_or_default();
                        if let Some(gt) = tag_text.find('>') {
                            let source_pos = tag_start + gt + 1 + abs_pos;
                            ctx.diagnostic(
                                format!("{} is already reactive, $state wrapping is unnecessary.", class_name),
                                oxc::span::Span::new(source_pos as u32, (source_pos + 7) as u32),
                            );
                        }
                    }
                }
                search_from = abs_pos + 7;
            }
        }
    }
}
