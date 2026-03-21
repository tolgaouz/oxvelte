//! `svelte/no-navigation-without-base` — require navigation functions to use base path.

use crate::linter::{parse_imports, walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute};

const NAV_FUNCTIONS: &[&str] = &["goto", "pushState", "replaceState"];

pub struct NoNavigationWithoutBase;

/// Check if a string value is an absolute URI or fragment (should not be flagged).
fn is_exempt_href(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
        || s.starts_with("mailto:") || s.starts_with("tel:")
        || s.starts_with("//") || s.starts_with('#')
        || s.is_empty()
}

impl Rule for NoNavigationWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Parse imports to find base and navigation functions
        let imports = if let Some(script) = &ctx.ast.instance {
            parse_imports(&script.content)
        } else {
            Vec::new()
        };

        let base_local: Option<String> = imports.iter()
            .find(|(_, imported, module)| {
                (imported == "base" || imported == "*") && module == "$app/paths"
            })
            .map(|(local, imported, _)| {
                if imported == "*" { format!("{}.base", local) } else { local.clone() }
            });

        // Check script for navigation function calls
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
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

            if !nav_local_names.is_empty() {
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
                            let is_absolute_uri = if let Some(end) = inner.find(quote as char) {
                                is_exempt_href(&inner[..end])
                            } else { false };

                            if !is_absolute_uri {
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
                                        "Use `base` from `$app/paths` when calling navigation functions with paths.",
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

        // Check <a> elements for href values that are paths without base
        let base_local_clone = base_local.clone();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "a" { return; }
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, span, .. } = attr {
                        if name != "href" { continue; }
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(eq_pos) = region.find('=') {
                            let val = region[eq_pos + 1..].trim();
                            // Plain quoted attribute: href="/path"
                            if (val.starts_with('"') && val.ends_with('"'))
                                || (val.starts_with('\'') && val.ends_with('\''))
                            {
                                let inner = &val[1..val.len()-1];
                                if inner.starts_with('/') && !is_exempt_href(inner) {
                                    ctx.diagnostic(
                                        "Use `base` from `$app/paths` in `<a>` href attributes with paths.",
                                        *span,
                                    );
                                }
                            }
                            // Expression: href={expr}
                            else if val.starts_with('{') && val.ends_with('}') {
                                let expr = val[1..val.len()-1].trim();

                                // Check if expression uses base AS A PREFIX
                                let uses_base = if let Some(ref bname) = base_local_clone {
                                    // base + '/path' or base+'/path'
                                    expr.starts_with(&format!("{} +", bname))
                                    || expr.starts_with(&format!("{}+", bname))
                                    // `${base}/path`
                                    || expr.starts_with(&format!("${{{}}}",  bname))
                                    || expr.starts_with(&format!("`${{{}}}", bname))
                                } else { false };

                                if uses_base { continue; }

                                // Check if it's a string literal starting with /
                                let is_path_literal = if let Some(q) = expr.chars().next() {
                                    if q == '\'' || q == '"' || q == '`' {
                                        if let Some(end) = expr[1..].find(q) {
                                            expr[1..end+1].starts_with('/')
                                        } else { false }
                                    } else { false }
                                } else { false };

                                // Check for concatenation expressions containing path strings
                                let has_path_concat = expr.contains("'/'") || expr.contains("\"/\"");

                                if is_path_literal || has_path_concat {
                                    ctx.diagnostic(
                                        "Use `base` from `$app/paths` in `<a>` href attributes with paths.",
                                        *span,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
