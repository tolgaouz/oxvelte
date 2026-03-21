//! `svelte/no-navigation-without-resolve` — disallow SvelteKit navigation calls
//! (`goto`, `pushState`, etc.) without using `$app/paths` `resolveRoute`.
//! ⭐ Recommended

use crate::linter::{parse_imports, LintContext, Rule};
use oxc::span::Span;

const NAV_FUNCTIONS: &[&str] = &["goto", "pushState", "replaceState"];

pub struct NoNavigationWithoutResolve;

impl Rule for NoNavigationWithoutResolve {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-resolve"
    }

    fn is_recommended(&self) -> bool {
        true
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

            // Check if resolveRoute is imported
            let resolve_local: Option<String> = imports.iter()
                .find(|(_, imported, module)| {
                    (imported == "resolveRoute" || imported == "*") && module == "$app/paths"
                })
                .map(|(local, imported, _)| {
                    if imported == "*" { format!("{}.resolveRoute", local) } else { local.clone() }
                });

            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            for (local_name, orig_name) in &nav_local_names {
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

                    // Check if the argument is a string literal (not empty)
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
                            // Check if resolveRoute is used in this call
                            let call_text = &content[abs..];
                            let call_end = call_text.find(')').unwrap_or(call_text.len());
                            let call_body = &call_text[search_pattern.len()..call_end];

                            let uses_resolve = if let Some(ref rname) = resolve_local {
                                call_body.contains(rname)
                            } else { false };

                            if !uses_resolve {
                                let source_pos = base + gt + 1 + abs;
                                ctx.diagnostic(
                                    format!(
                                        "Use `resolveRoute` from `$app/paths` instead of passing a raw string to `{}`.",
                                        orig_name
                                    ),
                                    Span::new(source_pos as u32, (source_pos + search_pattern.len()) as u32),
                                );
                            }
                        }
                    } else if trimmed.starts_with("resolve") || trimmed.starts_with("resolveRoute") {
                        // resolve() is being used — but check if it's the full argument
                        // (partial resolve like `resolve('/foo') + '/bar'` should also be flagged)
                        // For now, allow any use of resolve in the argument
                    } else {
                        // Variable argument — don't flag (could be a resolved value)
                    }

                    search_from = abs + search_pattern.len();
                }
            }
        }
    }
}
