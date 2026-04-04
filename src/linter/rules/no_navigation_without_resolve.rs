//! `svelte/no-navigation-without-resolve` — disallow SvelteKit navigation calls
//! (`goto`, `pushState`, etc.) without using `$app/paths` `resolveRoute`.
//! ⭐ Recommended

use crate::linter::{parse_imports, walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};
use oxc::span::Span;

const NAV_FUNCTIONS: &[&str] = &["goto", "pushState", "replaceState"];

fn is_absolute_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
        || s.starts_with("mailto:") || s.starts_with("tel:")
        || s.starts_with("//")
}

fn find_closing_paren(s: &str) -> usize {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 { return i; }
                depth -= 1;
            }
            _ => {}
        }
    }
    s.len()
}

pub struct NoNavigationWithoutResolve;

impl Rule for NoNavigationWithoutResolve {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-resolve"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let (ignore_goto, ignore_push_state, ignore_replace_state, ignore_links) = {
            let opts = ctx.config.options.as_ref()
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first());
            (
                opts.and_then(|v| v.get("ignoreGoto")).and_then(|v| v.as_bool()).unwrap_or(false),
                opts.and_then(|v| v.get("ignorePushState")).and_then(|v| v.as_bool()).unwrap_or(false),
                opts.and_then(|v| v.get("ignoreReplaceState")).and_then(|v| v.as_bool()).unwrap_or(false),
                opts.and_then(|v| v.get("ignoreLinks")).and_then(|v| v.as_bool()).unwrap_or(false),
            )
        };

        let imports = ctx.ast.instance.as_ref()
            .map(|s| parse_imports(&s.content))
            .unwrap_or_default();

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;

            let mut nav_local_names: Vec<(String, &str)> = Vec::new();
            for (local, imported, module) in &imports {
                if module == "$app/navigation" {
                    if imported == "*" {
                        for nav_fn in NAV_FUNCTIONS {
                            if is_nav_ignored_resolve(nav_fn, ignore_goto, ignore_push_state, ignore_replace_state) { continue; }
                            nav_local_names.push((format!("{}.{}", local, nav_fn), nav_fn));
                        }
                    } else if NAV_FUNCTIONS.contains(&imported.as_str()) {
                        if !is_nav_ignored_resolve(&imported, ignore_goto, ignore_push_state, ignore_replace_state) {
                            nav_local_names.push((local.clone(), imported.as_str()));
                        }
                    }
                }
            }

            if !nav_local_names.is_empty() {

            let resolve_local: Option<String> = imports.iter()
                .find(|(_, imported, module)| {
                    (imported == "resolve" || imported == "asset" || imported == "*") && module == "$app/paths"
                })
                .map(|(local, imported, _)| {
                    if imported == "*" { format!("{}.resolve", local) } else { local.clone() }
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

                    let should_flag = if trimmed.starts_with('\'') || trimmed.starts_with('"') || trimmed.starts_with('`') {
                        let quote = trimmed.as_bytes()[0];
                        let inner = &trimmed[1..];
                        let is_empty = inner.starts_with(quote as char);
                        let is_absolute_uri = inner.find(quote as char)
                            .map_or(false, |end| is_absolute_url(&inner[..end]));
                        if is_empty || is_absolute_uri { false }
                        else {
                            let call_text = &content[abs + search_pattern.len()..];
                            let call_body = &call_text[..find_closing_paren(call_text)];
                            !resolve_local.as_ref().is_some_and(|r| call_body.contains(r.as_str()))
                        }
                    } else if trimmed.starts_with("resolve") || trimmed.starts_with("asset")
                        || resolve_local.as_ref().is_some_and(|r| trimmed.starts_with(r.as_str())) {
                        let call_text = &content[abs + search_pattern.len()..];
                        let call_body = &call_text[..find_closing_paren(call_text)];
                        let mut d = 0i32;
                        call_body.chars().any(|ch| match ch {
                            '(' | '[' | '{' => { d += 1; false }
                            ')' | ']' | '}' => { if d > 0 { d -= 1; } false }
                            '+' if d == 0 => true, _ => false,
                        })
                    } else {
                        let var_ident = trimmed.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').next().unwrap_or("");
                        !var_ident.is_empty() && !is_value_safe(var_ident, content, 0)
                    };
                    if should_flag {
                        let source_pos = base + gt + 1 + abs;
                        ctx.diagnostic(
                            format!("Unexpected {}() call without resolve().", orig_name),
                            Span::new(source_pos as u32, (source_pos + search_pattern.len()) as u32),
                        );
                    }

                    search_from = abs + search_pattern.len();
                }
            }
            } // end if nav_local_names not empty
        }

        if ignore_links { return; }

        let has_any_imports = !imports.is_empty();
        let has_sveltekit_imports = imports.iter().any(|(_, _, module)| module.starts_with("$app/"));
        if has_any_imports && !has_sveltekit_imports { return; }

        let has_resolve = imports.iter().any(|(_, imported, module)| {
            (imported == "resolve" || imported == "asset" || imported == "*") && module == "$app/paths"
        });

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "a" { return; }

                let el_source = &ctx.source[el.span.start as usize..el.span.end as usize];
                let has_rel = el.attributes.iter().any(|a| {
                    if let Attribute::NormalAttribute { name, value, .. } = a {
                        if name == "rel" {
                            return match value {
                                AttributeValue::Static(v) => v.contains("external"),
                                AttributeValue::Expression(e) => e.contains("external") || e.trim() == "rel",
                                _ => false,
                            };
                        }
                    }
                    false
                });
                let has_external = has_rel || el_source.contains("{rel}");
                if has_external { return; }

                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span, .. } = attr {
                        if name != "href" { continue; }
                        let region = &ctx.source[span.start as usize..span.end as usize];

                        let skip = match value {
                            AttributeValue::Static(v) => {
                                is_absolute_url(v) || v.starts_with('#') || v.is_empty()
                            }
                            _ => false,
                        };
                        if skip { continue; }

                        if let AttributeValue::Expression(expr) = value {
                            let e = expr.trim();
                            if (e.starts_with("resolve") || e.starts_with("asset")) && e.ends_with(')') && !e.contains('+') { continue; }
                            if e.contains('(') && e.ends_with(')') && !e.contains('+') { continue; }
                            if e.starts_with("'http") || e.starts_with("\"http")
                                || e.starts_with("'#") || e.starts_with("\"#")
                                || e.starts_with("'mailto:") || e.starts_with("\"mailto:")
                                || e.starts_with("`#") || e.contains("'#'")
                                || e.starts_with("'//") || e.starts_with("\"//")
                                || (e.starts_with('`') && e.contains("://"))
                                || e.contains("://")
                                || e == "undefined" || e == "null"
                            { continue; }
                            if e.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                                let script_content = ctx.ast.instance.as_ref().map(|s| s.content.as_str()).unwrap_or("");
                                if is_value_safe(e, script_content, 0) { continue; }
                            }
                        }
                        if region.contains("resolve(") && !region.contains('+') { continue; }
                        if has_resolve && region.contains("$") { continue; }

                        ctx.diagnostic(
                            "Unexpected href link without resolve().",
                            *span,
                        );
                    }
                }
            }
        });
    }
}

fn is_value_safe(var_name: &str, script_content: &str, depth: usize) -> bool {
    if depth > 5 { return false; } // prevent infinite recursion
    for kw in &["const ", "let ", "var "] {
        let pattern = format!("{}{}", kw, var_name);
        if let Some(pos) = script_content.find(&pattern) {
            let rest = &script_content[pos + pattern.len()..];
            let rest = rest.trim_start();
            if !rest.starts_with('=') { continue; }
            let init = rest[1..].trim_start();
            let end = init.find(|c| c == ';' || c == '\n').unwrap_or(init.len());
            let init = init[..end].trim();
            if init.is_empty() { continue; }
            if init == "null" || init == "undefined" { return true; }
            if (init.contains("resolve") || init.contains("asset")) && !init.contains('+') { return true; }
            if init.starts_with("'http") || init.starts_with("\"http")
                || init.starts_with("'//") || init.starts_with("\"//")
                || init.starts_with("'mailto:") || init.starts_with("'tel:") { return true; }
            if init.starts_with("'#") || init.starts_with("\"#") { return true; }
            if init.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                return is_value_safe(init, script_content, depth + 1);
            }
            if init.contains("resolve") { return true; }
            if init.starts_with("$page") || init.starts_with("$app") { return true; }
            return false;
        }
    }
    false // variable not found or not initialized
}

fn is_nav_ignored_resolve(name: &str, ignore_goto: bool, ignore_push_state: bool, ignore_replace_state: bool) -> bool {
    match name {
        "goto" => ignore_goto,
        "pushState" => ignore_push_state,
        "replaceState" => ignore_replace_state,
        _ => false,
    }
}
