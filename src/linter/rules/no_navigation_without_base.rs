//! `svelte/no-navigation-without-base` — require navigation functions to use base path.

use crate::linter::{parse_imports, LintContext, Rule};

const NAV_FUNCTIONS: &[&str] = &["goto", "pushState", "replaceState"];

pub struct NoNavigationWithoutBase;

impl Rule for NoNavigationWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let imports = parse_imports(content);

            // Find local names for navigation functions
            let mut nav_local_names: Vec<(String, &str)> = Vec::new();
            for (local, imported, module) in &imports {
                if module == "$app/navigation" {
                    if imported == "*" {
                        for nav_fn in NAV_FUNCTIONS {
                            nav_local_names.push((format!("{}.{}", local, nav_fn), nav_fn));
                        }
                    } else if NAV_FUNCTIONS.contains(&imported.as_str()) {
                        nav_local_names.push((local.clone(), imported.as_str()));
                    }
                }
            }

            if nav_local_names.is_empty() { return; }

            // Find local name for base from $app/paths
            let base_local: Option<String> = imports.iter()
                .find(|(_, imported, module)| {
                    (imported == "base" || imported == "*") && module == "$app/paths"
                })
                .map(|(local, imported, _)| {
                    if imported == "*" { format!("{}.base", local) } else { local.clone() }
                });

            let script_base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[script_base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            for (local_name, _orig_name) in &nav_local_names {
                let search_pattern = format!("{}(", local_name);
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(&search_pattern) {
                    let abs = search_from + pos;
                    if abs > 0 {
                        let prev = content.as_bytes()[abs - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            search_from = abs + search_pattern.len();
                            continue;
                        }
                    }
                    let rest = &content[abs + search_pattern.len()..];
                    let trimmed = rest.trim_start();

                    if trimmed.starts_with('\'') || trimmed.starts_with('"') || trimmed.starts_with('`') {
                        let quote = trimmed.as_bytes()[0];
                        let inner = &trimmed[1..];
                        let is_empty = inner.starts_with(quote as char);
                        let is_absolute_uri = if let Some(end) = inner.find(quote as char) {
                            let s = &inner[..end];
                            s.starts_with("http://") || s.starts_with("https://")
                                || s.starts_with("mailto:") || s.starts_with("tel:")
                                || s.starts_with("//")
                        } else { false };

                        if !is_empty && !is_absolute_uri {
                            let call_text = &content[abs..];
                            let call_end = call_text.find(')').unwrap_or(call_text.len());
                            let call_body = &call_text[..call_end];
                            let uses_base = if let Some(ref bname) = base_local {
                                call_body.contains(&format!("`${{{}}}", bname)) ||
                                call_body.contains(&format!("{} +", bname)) ||
                                call_body.contains(&format!("{}+", bname))
                            } else { false };

                            if !uses_base {
                                let source_pos = script_base + gt + 1 + abs;
                                ctx.diagnostic(
                                    format!("Use `base` from `$app/paths` when calling navigation functions with paths."),
                                    oxc::span::Span::new(source_pos as u32, (source_pos + search_pattern.len()) as u32),
                                );
                            }
                        }
                    }

                    search_from = abs + search_pattern.len();
                }
            }
        }

        // Also check <a> tags for nullish-like href values
        // (handled by template walking, but keeping simple for now)
    }
}
