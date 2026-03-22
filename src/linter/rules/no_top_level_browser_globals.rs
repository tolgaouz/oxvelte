//! `svelte/no-top-level-browser-globals` — disallow top-level access to browser globals.

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

impl Rule for NoTopLevelBrowserGlobals {
    fn name(&self) -> &'static str {
        "svelte/no-top-level-browser-globals"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let script = match &ctx.ast.instance { Some(s) => s, None => return };
        if script.module { return; }
        let content = &script.content;
        let bytes = content.as_bytes();
        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        for global in BROWSER_GLOBALS {
            for (byte_offset, _) in content.match_indices(global) {
                // Word boundary check
                if byte_offset > 0 {
                    let p = bytes[byte_offset - 1];
                    if p.is_ascii_alphanumeric() || p == b'_' || p == b'$' || p == b'.' { continue; }
                }
                let after_pos = byte_offset + global.len();
                if after_pos < bytes.len() {
                    let a = bytes[after_pos];
                    if a.is_ascii_alphanumeric() || a == b'_' || a == b'$' { continue; }
                }

                // Only flag at depth 0 — simple brace count
                let depth: i32 = content[..byte_offset].bytes()
                    .fold(0i32, |acc, b| match b { b'{' => acc + 1, b'}' => acc - 1, _ => acc });
                if depth > 0 { continue; }

                // Skip if directly preceded by typeof
                let before = content[..byte_offset].trim_end();
                if before.ends_with("typeof") { continue; }

                // Extract current line
                let line_start = content[..byte_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line_end = content[byte_offset..].find('\n').map(|p| byte_offset + p).unwrap_or(content.len());
                let line = &content[line_start..line_end];

                // Skip import declarations and comments
                if line.trim_start().starts_with("import ") { continue; }
                if line.trim_start().starts_with("//") { continue; }
                // Skip import.meta guards
                if line.contains("import.meta") { continue; }

                // Check for VALID typeof guard: typeof X !== 'undefined' && X.prop
                // (but NOT: typeof X === 'undefined' && X.prop — wrong direction)
                if line.contains("typeof") {
                    let has_valid_typeof = line.contains(&format!("typeof {} !== ", global))
                        || line.contains(&format!("typeof {} != ", global));
                    if has_valid_typeof { continue; }
                    // If typeof check is wrong direction (=== undefined), DON'T skip
                }

                // Check for VALID globalThis guard: globalThis.X && X.prop
                // (but NOT: globalThis.X || X.prop — wrong direction)
                if line.contains(&format!("globalThis.{}", global)) {
                    // Optional chaining is always safe
                    if line.contains(&format!("globalThis.{}?.", global)) { continue; }
                    // globalThis.X && ... is a valid guard
                    let gt_pattern = format!("globalThis.{}", global);
                    if let Some(gt_pos) = line.find(&gt_pattern) {
                        let after_gt = &line[gt_pos + gt_pattern.len()..].trim_start();
                        if after_gt.starts_with("&&") { continue; }
                        if after_gt.starts_with("!==") || after_gt.starts_with("!=") { continue; }
                        // globalThis.X.prop (direct access) or || — DON'T skip
                    }
                }

                // Browser/BROWSER guard with && or ternary
                let has_browser = line.contains("browser") || line.contains("BROWSER");
                if has_browser && (line.contains('?') || line.contains("&&")) { continue; }

                let start = (content_offset + byte_offset) as u32;
                let end = start + global.len() as u32;
                ctx.diagnostic(
                    format!("Avoid referencing `{}` at the top level — it is not available during SSR.", global),
                    Span::new(start, end),
                );
            }
        }
    }
}
