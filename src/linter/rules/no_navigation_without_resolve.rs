//! `svelte/no-navigation-without-resolve` — disallow SvelteKit navigation calls
//! (`goto`, `pushState`, etc.) without using `$app/paths` `resolveRoute`.
//! ⭐ Recommended

use crate::linter::{parse_imports, walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};
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
        // Config options: ignoreGoto, ignorePushState, ignoreReplaceState, ignoreLinks
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

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let imports = parse_imports(content);

            // Find local names for navigation functions
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

            // Check if resolveRoute is imported
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
                            // Check if resolve is used in this call (balanced paren search)
                            let call_text = &content[abs + search_pattern.len()..];
                            let mut depth = 0i32;
                            let mut call_end = call_text.len();
                            for (ci, ch) in call_text.char_indices() {
                                match ch {
                                    '(' => depth += 1,
                                    ')' => {
                                        if depth == 0 { call_end = ci; break; }
                                        depth -= 1;
                                    }
                                    _ => {}
                                }
                            }
                            let call_body = &call_text[..call_end];

                            let uses_resolve = if let Some(ref rname) = resolve_local {
                                call_body.contains(rname)
                            } else { false };

                            if !uses_resolve {
                                let source_pos = base + gt + 1 + abs;
                                ctx.diagnostic(
                                    format!(
                                        "Unexpected {}() call without resolve().",
                                        orig_name
                                    ),
                                    Span::new(source_pos as u32, (source_pos + search_pattern.len()) as u32),
                                );
                            }
                        }
                    } else if trimmed.starts_with("resolve") || trimmed.starts_with("asset")
                        || resolve_local.as_ref().map_or(false, |r| trimmed.starts_with(r.as_str())) {
                        // resolve()/asset() at the start — check for concatenation
                        let call_text = &content[abs + search_pattern.len()..];
                        // Find the matching close paren of the outer navigation call
                        let mut depth = 0i32;
                        let mut outer_end = call_text.len();
                        for (i, ch) in call_text.char_indices() {
                            match ch {
                                '(' => depth += 1,
                                ')' => {
                                    if depth == 0 { outer_end = i; break; }
                                    depth -= 1;
                                }
                                _ => {}
                            }
                        }
                        let call_body = &call_text[..outer_end];
                        // Check for `+` at top level (concatenation)
                        let mut d = 0i32;
                        let has_concat = call_body.chars().any(|ch| {
                            match ch {
                                '(' | '[' | '{' => { d += 1; false }
                                ')' | ']' | '}' => { if d > 0 { d -= 1; } false }
                                '+' if d == 0 => true,
                                _ => false,
                            }
                        });
                        if has_concat {
                            let source_pos = base + gt + 1 + abs;
                            ctx.diagnostic(
                                format!("Unexpected {}() call without resolve().", orig_name),
                                Span::new(source_pos as u32, (source_pos + search_pattern.len()) as u32),
                            );
                        }
                    } else {
                        // Variable argument — trace to its initializer
                        let var_ident = trimmed.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').next().unwrap_or("");
                        if !var_ident.is_empty() && !is_value_safe(var_ident, content, 0) {
                            let source_pos = base + gt + 1 + abs;
                            ctx.diagnostic(
                                format!("Unexpected {}() call without resolve().", orig_name),
                                Span::new(source_pos as u32, (source_pos + search_pattern.len()) as u32),
                            );
                        }
                    }

                    search_from = abs + search_pattern.len();
                }
            }
            } // end if nav_local_names not empty
        }

        // If ignoreLinks is set, skip template <a> checking
        if ignore_links { return; }

        // Template <a href> checking: trace variable values to check if safe
        // Only check when the file uses SvelteKit ($app/* imports indicate SvelteKit)
        let imports = if let Some(script) = &ctx.ast.instance {
            parse_imports(&script.content)
        } else { Vec::new() };

        // Only check <a href> if the file looks like a SvelteKit component.
        // Skip if there are non-$app imports but no $app imports (clearly not SvelteKit).
        // Still check if there are no imports at all (could be a simple SvelteKit page).
        let has_any_imports = !imports.is_empty();
        let has_sveltekit_imports = imports.iter().any(|(_, _, module)| module.starts_with("$app/"));
        if has_any_imports && !has_sveltekit_imports { return; }

        let has_resolve = imports.iter().any(|(_, imported, module)| {
            (imported == "resolve" || imported == "asset" || imported == "*") && module == "$app/paths"
        });

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "a" { return; }

                // Check for rel="external" — skip links with rel containing external
                // Also handle shorthand {rel} where rel variable is 'external'
                let el_source = &ctx.source[el.span.start as usize..el.span.end as usize];
                let has_rel = el.attributes.iter().any(|a| {
                    match a {
                        Attribute::NormalAttribute { name, value, .. } => {
                            if name == "rel" {
                                return match value {
                                    AttributeValue::Static(v) => v.contains("external"),
                                    AttributeValue::Expression(e) => e.contains("external") || e.trim() == "rel",
                                    _ => false,
                                };
                            }
                            // Shorthand {rel}
                            if name == "rel" || (matches!(value, AttributeValue::Expression(e) if e.trim() == "rel")) {
                                return true;
                            }
                            false
                        }
                        _ => false,
                    }
                });
                // Also check the raw element source for {rel} shorthand
                let has_external = has_rel || el_source.contains("{rel}");
                if has_external { return; }

                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span, .. } = attr {
                        if name != "href" { continue; }
                        let region = &ctx.source[span.start as usize..span.end as usize];

                        // Skip absolute URLs and fragments
                        let skip = match value {
                            AttributeValue::Static(v) => {
                                v.starts_with("http://") || v.starts_with("https://")
                                    || v.starts_with("mailto:") || v.starts_with("tel:")
                                    || v.starts_with("//") || v.starts_with('#') || v.is_empty()
                            }
                            _ => false,
                        };
                        if skip { continue; }

                        // Check if resolve/asset wraps the ENTIRE value (not partial)
                        if let AttributeValue::Expression(expr) = value {
                            let e = expr.trim();
                            // resolve('/path') or asset('/path') as the entire expression
                            if (e.starts_with("resolve") || e.starts_with("asset")) && e.ends_with(')') && !e.contains('+') {
                                continue;
                            }
                        }
                        if region.contains("resolve(") && !region.contains('+') { continue; }
                        if has_resolve && region.contains("$") { continue; }
                        // Check if expression is a PURE function call (not concatenated)
                        if let AttributeValue::Expression(expr) = value {
                            let e = expr.trim();
                            // Pure call: `fn(args)` — no concatenation
                            if e.contains('(') && e.ends_with(')') && !e.contains('+') { continue; }
                        }
                        // Skip if the expression is clearly safe (fragment, absolute URL, etc.)
                        if let AttributeValue::Expression(expr) = value {
                            let e = expr.trim();
                            if e.starts_with("'http") || e.starts_with("\"http")
                                || e.starts_with("'#") || e.starts_with("\"#")
                                || e.starts_with("'mailto:") || e.starts_with("\"mailto:")
                                || e.starts_with("`#") || e.contains("'#'")
                                || e.starts_with("'//") || e.starts_with("\"//")
                                // Template literal with URL protocol
                                || (e.starts_with('`') && e.contains("://"))
                                // Expression producing URL (contains protocol)
                                || e.contains("://")
                                // Nullish
                                || e == "undefined" || e == "null"
                                {
                                continue;
                            }
                        }
                        // For identifiers, trace to initializer value
                        if let AttributeValue::Expression(expr) = value {
                            let e = expr.trim();
                            if e.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                                // Trace variable to its initializer
                                let script_content = ctx.ast.instance.as_ref().map(|s| s.content.as_str()).unwrap_or("");
                                if is_value_safe(e, script_content, 0) { continue; }
                            }
                        }

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

/// Trace a variable name to its initializer and check if the value is safe
/// (resolve call, absolute URL, fragment, nullish, or asset call).
fn is_value_safe(var_name: &str, script_content: &str, depth: usize) -> bool {
    if depth > 5 { return false; } // prevent infinite recursion
    // Find `const/let VAR = INIT;` in script
    for kw in &["const ", "let ", "var "] {
        let pattern = format!("{}{}", kw, var_name);
        if let Some(pos) = script_content.find(&pattern) {
            let rest = &script_content[pos + pattern.len()..];
            let rest = rest.trim_start();
            if !rest.starts_with('=') { continue; }
            let init = rest[1..].trim_start();
            // Get the initializer up to ; or newline
            let end = init.find(|c| c == ';' || c == '\n').unwrap_or(init.len());
            let init = init[..end].trim();
            // Check if initializer is safe
            if init.is_empty() { continue; }
            // null/undefined → safe
            if init == "null" || init == "undefined" { return true; }
            // resolve()/asset() call → safe only if it's a pure call (no concatenation)
            if (init.contains("resolve") || init.contains("asset")) && !init.contains('+') { return true; }
            // Absolute URL → safe
            if init.starts_with("'http") || init.starts_with("\"http")
                || init.starts_with("'//") || init.starts_with("\"//")
                || init.starts_with("'mailto:") || init.starts_with("'tel:") { return true; }
            // Fragment → safe
            if init.starts_with("'#") || init.starts_with("\"#") { return true; }
            // Another identifier → trace recursively
            if init.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                return is_value_safe(init, script_content, depth + 1);
            }
            // Template literal with resolve → safe
            if init.contains("resolve") { return true; }
            // $page.url, $page.data → safe (SvelteKit stores)
            if init.starts_with("$page") || init.starts_with("$app") { return true; }
            // Not clearly safe
            return false;
        }
    }
    false // variable not found or not initialized
}

/// Check if a navigation function should be ignored based on config.
fn is_nav_ignored_resolve(name: &str, ignore_goto: bool, ignore_push_state: bool, ignore_replace_state: bool) -> bool {
    match name {
        "goto" => ignore_goto,
        "pushState" => ignore_push_state,
        "replaceState" => ignore_replace_state,
        _ => false,
    }
}
