//! `svelte/no-navigation-without-base` — require navigation functions to use base path.

use crate::linter::{parse_imports, walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute};

const NAV_FUNCTIONS: &[&str] = &["goto", "pushState", "replaceState"];

pub struct NoNavigationWithoutBase;

fn is_nav_ignored(name: &str, ignore_goto: bool, ignore_push_state: bool, ignore_replace_state: bool) -> bool {
    match name {
        "goto" => ignore_goto,
        "pushState" => ignore_push_state,
        "replaceState" => ignore_replace_state,
        _ => false,
    }
}

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
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array()).and_then(|arr| arr.first());
        let get_bool = |key: &str| opts.and_then(|v| v.get(key)).and_then(|v| v.as_bool()).unwrap_or(false);
        let ignore_goto = get_bool("ignoreGoto");
        let ignore_push_state = get_bool("ignorePushState");
        let ignore_replace_state = get_bool("ignoreReplaceState");
        let ignore_links = get_bool("ignoreLinks");

        let imports = ctx.ast.instance.as_ref().map(|s| parse_imports(&s.content)).unwrap_or_default();

        let base_local: Option<String> = imports.iter()
            .find(|(_, imported, module)| {
                (imported == "base" || imported == "*") && module == "$app/paths"
            })
            .map(|(local, imported, _)| {
                if imported == "*" { format!("{}.base", local) } else { local.clone() }
            });

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let mut nav_local_names: Vec<(String, &str)> = Vec::new();
            for (local, imported, module) in &imports {
                if module == "$app/navigation" {
                    if imported == "*" {
                        for nav_fn in NAV_FUNCTIONS {
                            if is_nav_ignored(nav_fn, ignore_goto, ignore_push_state, ignore_replace_state) { continue; }
                            nav_local_names.push((format!("{}.{}", local, nav_fn), nav_fn));
                        }
                    } else if NAV_FUNCTIONS.contains(&imported.as_str()) {
                        if !is_nav_ignored(&imported, ignore_goto, ignore_push_state, ignore_replace_state) {
                            nav_local_names.push((local.clone(), imported.as_str()));
                        }
                    }
                }
            }

            if !nav_local_names.is_empty() {
                let script_base = script.span.start as usize;
                let source = ctx.source;
                let tag_text = &source[script_base..script.span.end as usize];
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

                        if matches!(trimmed.as_bytes().first(), Some(b'\'' | b'"' | b'`')) {
                            let quote = trimmed.as_bytes()[0];
                            let is_absolute_uri = trimmed[1..].find(quote as char)
                                .map_or(false, |end| is_exempt_href(&trimmed[1..end+1]));

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
                                        format!("Found a {}() call with a url that isn't prefixed with the base path.", orig_name),
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

        if ignore_links { return; }

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "a" { return; }
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, span, .. } = attr {
                        if name != "href" { continue; }
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(eq_pos) = region.find('=') {
                            let val = region[eq_pos + 1..].trim();
                            if matches!((val.as_bytes().first(), val.as_bytes().last()),
                                (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\''))) {
                                let inner = &val[1..val.len()-1];
                                if inner.starts_with('/') && !is_exempt_href(inner) {
                                    ctx.diagnostic("Found a link with a url that isn't prefixed with the base path.", *span);
                                }
                            }
                            else if val.starts_with('{') && val.ends_with('}') {
                                let expr = val[1..val.len()-1].trim();

                                let uses_base = if let Some(ref bname) = base_local {
                                    expr.starts_with(&format!("{} +", bname))
                                    || expr.starts_with(&format!("{}+", bname))
                                    || expr.starts_with(&format!("${{{}}}",  bname))
                                    || expr.starts_with(&format!("`${{{}}}", bname))
                                } else { false };

                                if uses_base { continue; }

                                let is_path_literal = matches!(expr.as_bytes().first(), Some(b'\'' | b'"' | b'`'))
                                    && expr[1..].find(expr.as_bytes()[0] as char)
                                        .map_or(false, |e| expr[1..e+1].starts_with('/'));

                                let has_path_concat = expr.contains("'/'") || expr.contains("\"/\"");

                                if is_path_literal || has_path_concat {
                                    ctx.diagnostic(
                                        "Found a link with a url that isn't prefixed with the base path.",
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
