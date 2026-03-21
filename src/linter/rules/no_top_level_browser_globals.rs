//! `svelte/no-top-level-browser-globals` — disallow top-level access to browser globals
//! such as `window`, `document`, or `localStorage` outside lifecycle hooks.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

const BROWSER_GLOBALS: &[&str] = &[
    "window", "document", "navigator", "localStorage", "sessionStorage",
    "location", "history", "alert", "confirm", "prompt", "fetch",
    "XMLHttpRequest", "requestAnimationFrame", "cancelAnimationFrame",
    "setTimeout", "setInterval", "clearTimeout", "clearInterval",
    "customElements", "getComputedStyle", "matchMedia",
    "IntersectionObserver", "MutationObserver", "ResizeObserver",
];

pub struct NoTopLevelBrowserGlobals;

/// Compute brace depth at each position in the script content.
/// Returns depth for each byte position. Properly handles strings and comments.
fn compute_brace_depths(content: &str) -> Vec<i32> {
    let bytes = content.as_bytes();
    let mut depths = vec![0i32; content.len()];
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        // Skip line comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                depths[i] = depth;
                i += 1;
            }
            continue;
        }
        // Skip block comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            while i < bytes.len() {
                depths[i] = depth;
                if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 1;
                    depths[i] = depth;
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Skip strings
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let q = bytes[i];
            depths[i] = depth;
            i += 1;
            while i < bytes.len() && bytes[i] != q {
                depths[i] = depth;
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                    depths[i] = depth;
                }
                i += 1;
            }
            if i < bytes.len() { depths[i] = depth; i += 1; }
            continue;
        }
        // Template literals
        if bytes[i] == b'`' {
            depths[i] = depth;
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' {
                depths[i] = depth;
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                    depths[i] = depth;
                } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    // Template expression - track depth
                    i += 1; depths[i] = depth;
                    i += 1; depth += 1;
                    continue;
                }
                i += 1;
            }
            if i < bytes.len() { depths[i] = depth; i += 1; }
            continue;
        }
        // Track braces
        depths[i] = match bytes[i] {
            b'{' => { let d = depth; depth += 1; d }
            b'}' => { depth -= 1; if depth < 0 { depth = 0; } depth }
            _ => depth,
        };
        i += 1;
    }
    depths
}

impl Rule for NoTopLevelBrowserGlobals {
    fn name(&self) -> &'static str {
        "svelte/no-top-level-browser-globals"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            if script.module { return; }
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

            let depths = compute_brace_depths(content);

            for global in BROWSER_GLOBALS {
                for (byte_offset, _) in content.match_indices(global) {
                    // Word boundary check
                    if byte_offset > 0 {
                        let p = content.as_bytes()[byte_offset - 1];
                        if p.is_ascii_alphanumeric() || p == b'_' || p == b'$' || p == b'.' { continue; }
                    }
                    let after_pos = byte_offset + global.len();
                    if after_pos < content.len() {
                        let a = content.as_bytes()[after_pos];
                        if a.is_ascii_alphanumeric() || a == b'_' || a == b'$' { continue; }
                    }

                    // Only flag at depth 0
                    if byte_offset < depths.len() && depths[byte_offset] > 0 { continue; }
                    // Double-check: count braces before this position
                    let manual_depth = content[..byte_offset].chars()
                        .fold(0i32, |d, c| match c { '{' => d + 1, '}' => d - 1, _ => d });
                    if manual_depth > 0 { continue; }

                    // Skip if preceded by typeof
                    let before = content[..byte_offset].trim_end();
                    if before.ends_with("typeof") { continue; }

                    // Skip line-level guards
                    let line_start = content[..byte_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let line_end = content[byte_offset..].find('\n').map(|p| byte_offset + p).unwrap_or(content.len());
                    let line = &content[line_start..line_end];

                    // typeof guard on same line
                    if line.contains("typeof") { continue; }
                    // globalThis guard on same line
                    if line.contains(&format!("globalThis.{}", global)) { continue; }
                    // browser guard (browser, BROWSER from esm-env)
                    let line_lower = line.to_lowercase();
                    if line_lower.contains("browser") && (line.contains('?') || line.contains("&&")) { continue; }
                    // import.meta guard
                    if line.contains("import.meta") { continue; }
                    // import declaration
                    if line.trim_start().starts_with("import ") { continue; }

                    let start = (content_offset + byte_offset) as u32;
                    let end = start + global.len() as u32;
                    ctx.diagnostic(
                        format!(
                            "Avoid referencing `{}` at the top level — it is not available during SSR. Use `onMount` or a browser check.",
                            global
                        ),
                        Span::new(start, end),
                    );
                }
            }
        }
    }
}
