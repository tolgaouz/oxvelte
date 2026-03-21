//! `svelte/no-goto-without-base` — require goto to use base path.

use crate::linter::{parse_imports, LintContext, Rule};

pub struct NoGotoWithoutBase;

impl Rule for NoGotoWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-goto-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let imports = parse_imports(content);

            // Find how goto is imported (direct or aliased)
            let goto_local_names: Vec<String> = imports.iter()
                .filter(|(_, imported, module)| {
                    (imported == "goto" || imported == "*") && module == "$app/navigation"
                })
                .map(|(local, imported, _)| {
                    if imported == "*" {
                        format!("{}.goto", local)
                    } else {
                        local.clone()
                    }
                })
                .collect();

            if goto_local_names.is_empty() { return; }

            // Find local name for base from $app/paths
            let base_local: Option<String> = imports.iter()
                .find(|(_, imported, module)| {
                    (imported == "base" || imported == "*") && module == "$app/paths"
                })
                .map(|(local, imported, _)| {
                    if imported == "*" { format!("{}.base", local) } else { local.clone() }
                });

            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);

            for goto_name in &goto_local_names {
                let search_pattern = format!("{}(", goto_name);
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(&search_pattern) {
                    let abs = search_from + pos;
                    // Word boundary check
                    if abs > 0 {
                        let prev = content.as_bytes()[abs - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            search_from = abs + search_pattern.len();
                            continue;
                        }
                    }
                    let rest = &content[abs + search_pattern.len()..];
                    let trimmed = rest.trim_start();

                    // Check if the argument is a string literal (not an absolute URI)
                    if trimmed.starts_with('\'') || trimmed.starts_with('"') || trimmed.starts_with('`') {
                        let quote = trimmed.as_bytes()[0];
                        let inner = &trimmed[1..];
                        let is_absolute_uri = if let Some(end) = inner.find(quote as char) {
                            let s = &inner[..end];
                            s.starts_with("http://") || s.starts_with("https://")
                                || s.starts_with("mailto:") || s.starts_with("tel:")
                                || s.starts_with("//")
                        } else { false };

                        if !is_absolute_uri {
                            // Check if base is used as prefix in this goto call
                            let call_text = &content[abs..];
                            let call_end = call_text.find(')').unwrap_or(call_text.len());
                            let call_body = &call_text[..call_end];
                            let uses_base = if let Some(ref base_name) = base_local {
                                call_body.contains(&format!("`${{{}}}", base_name)) ||
                                call_body.contains(&format!("{} +", base_name)) ||
                                call_body.contains(&format!("{}+", base_name))
                            } else { false };

                            if !uses_base {
                                let source_pos = base + gt + 1 + abs;
                                ctx.diagnostic(
                                    format!("Use `base` from `$app/paths` when calling `goto` with an absolute path."),
                                    oxc::span::Span::new(source_pos as u32, (source_pos + search_pattern.len()) as u32),
                                );
                            }
                        }
                    }

                    search_from = abs + search_pattern.len();
                }
            }
        }
    }
}
