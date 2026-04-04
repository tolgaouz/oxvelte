//! `svelte/no-top-level-browser-globals` — disallow top-level access to browser globals.

use crate::linter::{LintContext, Rule};
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

fn is_in_ssr_block(content: &str, pos: usize) -> bool {
    let before = &content[..pos];
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

    if before_brace.ends_with("else") || before_brace.ends_with("else ") {
        let before_else = before_brace.trim_end().strip_suffix("else").unwrap_or(before_brace).trim_end();
        if before_else.ends_with('}') {
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
                        let cond_t = cond.trim();
                        let is_positive_browser = (cond_t.starts_with("globalThis.")
                            && !["=== undefined", "== undefined", "=== null", "== null"].iter().any(|p| cond_t.contains(p)))
                            || cond_t == "browser" || cond_t == "BROWSER"
                            || cond_t.ends_with(".BROWSER") || cond_t.ends_with(".browser")
                            || (cond_t.contains("typeof") && (cond_t.contains("!==") || cond_t.contains("!=")) && cond_t.contains("undefined"));
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

fn is_after_browser_guard_exit(content: &str, pos: usize) -> bool {
    let before = content[..pos].trim_end();
    let mut depth = 0i32;
    for i in (0..before.len()).rev() {
        match before.as_bytes()[i] {
            b'}' if depth == 0 => {
                let block_before = &before[..=i];
                let mut d = 0i32;
                let mut open = None;
                for (j, b) in block_before.bytes().enumerate().rev() {
                    match b {
                        b'}' => d += 1,
                        b'{' => { d -= 1; if d == 0 { open = Some(j); break; } }
                        _ => {}
                    }
                }
                if let Some(open_pos) = open {
                    let block_content = &content[open_pos + 1..i];
                    let has_jump = block_content.contains("continue")
                        || block_content.contains("return")
                        || block_content.contains("break");
                    if has_jump {
                        let before_open = content[..open_pos].trim_end();
                        if before_open.ends_with(')') {
                            if let Some(paren_start) = before_open.rfind('(') {
                                let cond = &before_open[paren_start + 1..before_open.len() - 1];
                                let is_browser = cond.trim() == "browser" || cond.trim() == "BROWSER"
                                    || cond.trim().contains("browser") || cond.trim().contains("BROWSER")
                                    || cond.trim().starts_with("globalThis.");
                                let is_negated = cond.trim().starts_with('!');
                                let kw = before_open[..paren_start].trim_end();
                                if kw.ends_with("if") {
                                    if is_browser && !is_negated { return true; }
                                }
                            }
                        }
                    }
                }
                return false; // only check the nearest block
            }
            b'}' => depth += 1,
            b'{' => depth -= 1,
            _ => {}
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
        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        for global in BROWSER_GLOBALS {
            for (byte_offset, _) in content.match_indices(global) {
                if !is_word_boundary(content, byte_offset, global.len()) { continue; }

                let depth: i32 = content[..byte_offset].bytes()
                    .fold(0i32, |acc, b| match b { b'{' => acc + 1, b'}' => acc - 1, _ => acc });
                if depth > 0 {
                    let in_ssr = is_in_ssr_block(content, byte_offset);
                    let after_exit = is_after_browser_guard_exit(content, byte_offset);
                    if !in_ssr && !after_exit { continue; }
                }

                let before = content[..byte_offset].trim_end();
                if before.ends_with("typeof") { continue; }

                let line_start = content[..byte_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line_end = content[byte_offset..].find('\n').map(|p| byte_offset + p).unwrap_or(content.len());
                let line = &content[line_start..line_end];

                let lt = line.trim_start();
                if lt.starts_with("import ") || lt.starts_with("//") || line.contains("import.meta") { continue; }
                if line.contains(&format!("typeof {} !== ", global)) || line.contains(&format!("typeof {} != ", global)) { continue; }

                let gt_pat = format!("globalThis.{}", global);
                if let Some(gt_pos) = line.find(&gt_pat) {
                    if line.contains(&format!("globalThis.{}?.", global)) { continue; }
                    let after = line[gt_pos + gt_pat.len()..].trim_start();
                    if after.starts_with("&&") || after.starts_with("!==") || after.starts_with("!=") { continue; }
                }

                let has_browser = line.contains("browser") || line.contains("BROWSER");
                if has_browser {
                    let global_pos_in_line = byte_offset - line_start;
                    let before_global = &line[..global_pos_in_line];
                    let has_neg = before_global.contains("!browser") || before_global.contains("!BROWSER");

                    if before_global.contains("&&") && !has_neg { continue; }

                    if line.contains('?') {
                        let q_pos = line.find('?').unwrap_or(line.len());
                        if q_pos < global_pos_in_line {
                            let colon_pos = line[q_pos..].find(':').map(|p| q_pos + p).unwrap_or(line.len());
                            let in_true_branch = global_pos_in_line < colon_pos;
                            let positive_guard = !line[..q_pos].contains('!');
                            if (in_true_branch && positive_guard) || (!in_true_branch && !positive_guard) {
                                continue;
                            }
                        }
                    }
                }

                let s = (content_offset + byte_offset) as u32;
                ctx.diagnostic(format!("Unexpected top-level browser global variable \"{}\".", global),
                    Span::new(s, s + global.len() as u32));
            }
        }

        check_template_nodes(&ctx.ast.html.nodes, ctx, false);
    }
}

fn check_template_nodes(nodes: &[TemplateNode], ctx: &mut LintContext<'_>, in_browser_ctx: bool) {
    for node in nodes {
        match node {
            TemplateNode::MustacheTag(tag) => {
                if !in_browser_ctx {
                    check_expr_for_globals(&tag.expression, tag.span, ctx);
                }
            }
            TemplateNode::IfBlock(block) => {
                let cond = block.test.trim();

                let is_browser_guard = cond.contains("browser") || cond.contains("BROWSER")
                    || cond.starts_with("typeof window") || cond.starts_with("typeof document")
                    || cond.starts_with("globalThis.");
                let is_negated = cond.starts_with('!');

                let cons_browser = in_browser_ctx || (is_browser_guard && !is_negated);
                let cons_server = is_browser_guard && is_negated;
                check_template_nodes(&block.consequent.nodes, ctx, cons_browser || (!cons_server && in_browser_ctx));

                if let Some(alt) = &block.alternate {
                    let alt_browser = in_browser_ctx || (is_browser_guard && is_negated);
                    if let TemplateNode::IfBlock(else_if) = alt.as_ref() {
                        let else_test = else_if.test.trim();
                        if else_test.is_empty() {
                            check_template_nodes(&else_if.consequent.nodes, ctx, alt_browser);
                        } else {
                            let ebc = else_test == "browser" || else_test == "BROWSER"
                                || else_test.contains("globalThis.");
                            let eng = else_test.starts_with('!');
                            let eb = alt_browser || (ebc && !eng);
                            check_template_nodes(&else_if.consequent.nodes, ctx, eb);
                            if let Some(a2) = &else_if.alternate {
                                let eb2 = alt_browser || (ebc && eng);
                                if let TemplateNode::IfBlock(a2if) = a2.as_ref() {
                                    check_template_nodes(&a2if.consequent.nodes, ctx, eb2);
                                }
                            }
                        }
                    }
                }
            }
            TemplateNode::Element(el) => {
                check_template_nodes(&el.children, ctx, in_browser_ctx);
            }
            TemplateNode::EachBlock(block) => {
                check_template_nodes(&block.body.nodes, ctx, in_browser_ctx);
                if let Some(fb) = &block.fallback {
                    check_template_nodes(&fb.nodes, ctx, in_browser_ctx);
                }
            }
            TemplateNode::KeyBlock(block) => {
                check_template_nodes(&block.body.nodes, ctx, in_browser_ctx);
            }
            TemplateNode::SnippetBlock(block) => {
                check_template_nodes(&block.body.nodes, ctx, in_browser_ctx);
            }
            _ => {}
        }
    }
}

fn is_word_boundary(text: &str, pos: usize, len: usize) -> bool {
    let bytes = text.as_bytes();
    (pos == 0 || !matches!(bytes[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'.'))
        && (pos + len >= bytes.len() || !matches!(bytes[pos + len], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'))
}

fn check_expr_for_globals(expr: &str, span: Span, ctx: &mut LintContext<'_>) {
    for global in BROWSER_GLOBALS {
        if let Some(pos) = expr.find(global) {
            if !is_word_boundary(expr, pos, global.len()) { continue; }
            ctx.diagnostic(format!("Unexpected top-level browser global variable \"{}\".", global), span);
        }
    }
}
