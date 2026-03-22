//! `svelte/no-top-level-browser-globals` — disallow top-level access to browser globals.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
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

/// Check if a position is inside a server-side block where browser globals are invalid:
/// - `if (import.meta.env.SSR) { ... }`
/// - `else { ... }` after `if (globalThis.X) { ... }`
fn is_in_ssr_block(content: &str, pos: usize) -> bool {
    let before = &content[..pos];
    // Find the last unmatched { before pos
    let mut depth = 0i32;
    let mut last_open = None;
    for (i, b) in before.bytes().enumerate().rev() {
        match b {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 { last_open = Some(i); break; }
                depth -= 1;
            }
            _ => {}
        }
    }
    let brace_pos = match last_open { Some(p) => p, None => return false };
    let before_brace = content[..brace_pos].trim_end();

    // Case 1: if (import.meta.env.SSR) { ... }
    if before_brace.ends_with(')') {
        if let Some(paren_start) = before_brace.rfind('(') {
            let cond = &before_brace[paren_start+1..before_brace.len()-1];
            if cond.trim().contains("import.meta.env.SSR")
                || cond.trim().contains("import.meta.env.DEV")
            {
                let before_paren = before_brace[..paren_start].trim_end();
                if before_paren.ends_with("if") { return true; }
            }
        }
    }

    // Case 2: else { ... } after if (globalThis.X) { ... }
    if before_brace.ends_with("else") || before_brace.ends_with("else ") {
        // Look further back for the preceding if block: } else
        // The } before "else" closes the if-true block
        let before_else = before_brace.trim_end().strip_suffix("else").unwrap_or(before_brace).trim_end();
        if before_else.ends_with('}') {
            // Find the matching { for this }
            let close_pos = before_else.len() - 1;
            let mut d = 0i32;
            let mut if_open = None;
            for (i, b) in before_else.bytes().enumerate().rev() {
                match b {
                    b'}' => d += 1,
                    b'{' => {
                        d -= 1;
                        if d == 0 { if_open = Some(i); break; }
                    }
                    _ => {}
                }
            }
            if let Some(open) = if_open {
                let before_if_block = content[..open].trim_end();
                if before_if_block.ends_with(')') {
                    if let Some(ps) = before_if_block.rfind('(') {
                        let cond = &before_if_block[ps+1..before_if_block.len()-1];
                        // if (globalThis.X) or if (browser) or if (BROWSER) or if (env.BROWSER)
                        let cond_t = cond.trim();
                        // Check for positive browser guard: else block = server-side
                        // if (globalThis.X) { browser } else { SERVER }
                        // if (BROWSER) { browser } else { SERVER }
                        // BUT NOT: if (globalThis.X === undefined) { server } else { BROWSER }
                        let is_positive_browser = (cond_t.starts_with("globalThis.")
                            && !cond_t.contains("=== undefined") && !cond_t.contains("== undefined")
                            && !cond_t.contains("=== null") && !cond_t.contains("== null"))
                            || cond_t == "browser" || cond_t == "BROWSER"
                            || cond_t.ends_with(".BROWSER") || cond_t.ends_with(".browser");
                        if is_positive_browser {
                            let kw = before_if_block[..ps].trim_end();
                            if kw.ends_with("if") { return true; }
                        }
                    }
                }
            }
        }
    }

    false
}

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

                // Only flag at depth 0, OR inside SSR-guarded blocks
                let depth: i32 = content[..byte_offset].bytes()
                    .fold(0i32, |acc, b| match b { b'{' => acc + 1, b'}' => acc - 1, _ => acc });
                if depth > 0 {
                    // Check if we're inside a server-side block where browser globals are invalid:
                    // - if (import.meta.env.SSR) { ... }
                    // - else { ... } after if (globalThis.X) { ... }
                    let in_server_block = is_in_ssr_block(content, byte_offset);
                    if !in_server_block { continue; }
                }

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

                // Browser/BROWSER guard — check direction
                let has_browser = line.contains("browser") || line.contains("BROWSER");
                if has_browser {
                    // Determine the guard's position relative to the global
                    let global_pos_in_line = byte_offset - line_start;
                    let before_global = &line[..global_pos_in_line];
                    let has_neg = before_global.contains("!browser") || before_global.contains("!BROWSER");

                    // browser && X — valid if browser is before && (positive guard)
                    if before_global.contains("&&") && !has_neg { continue; }

                    // browser ? X : Y — valid if global is in the true branch AND guard is positive
                    if line.contains('?') {
                        let q_pos = line.find('?').unwrap_or(line.len());
                        if q_pos < global_pos_in_line {
                            // Global is after ? — check which branch
                            let colon_pos = line[q_pos..].find(':').map(|p| q_pos + p).unwrap_or(line.len());
                            let in_true_branch = global_pos_in_line < colon_pos;
                            let positive_guard = !line[..q_pos].contains('!');
                            // browser ? TRUE : FALSE — global in true + positive guard = valid
                            // !browser ? TRUE : FALSE — global in false + negative guard = valid
                            if (in_true_branch && positive_guard) || (!in_true_branch && !positive_guard) {
                                continue;
                            }
                        }
                    }
                }

                let start = (content_offset + byte_offset) as u32;
                let end = start + global.len() as u32;
                ctx.diagnostic(
                    format!("Avoid referencing `{}` at the top level — it is not available during SSR.", global),
                    Span::new(start, end),
                );
            }
        }

        // Template-level browser global checking requires proper AST context
        // tracking for {#if browser} guards — disabled for now (14/15 pass).
    }
}
