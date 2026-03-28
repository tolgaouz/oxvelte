//! `svelte/no-inspect` — disallow use of `$inspect`.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

fn is_in_comment(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut in_single_line = false;
    let mut in_multi_line = false;
    let mut in_str = false;
    let mut str_ch = 0u8;

    while i < pos {
        if in_single_line {
            if bytes[i] == b'\n' { in_single_line = false; }
            i += 1;
            continue;
        }
        if in_multi_line {
            if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                in_multi_line = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_str {
            if bytes[i] == b'\\' { i += 2; continue; }
            if bytes[i] == str_ch { in_str = false; }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'\'' | b'"' | b'`' => { in_str = true; str_ch = bytes[i]; i += 1; }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => { in_single_line = true; i += 2; }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => { in_multi_line = true; i += 2; }
            _ => { i += 1; }
        }
    }
    in_single_line || in_multi_line
}

pub struct NoInspect;

impl Rule for NoInspect {
    fn name(&self) -> &'static str {
        "svelte/no-inspect"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for script in [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten() {
            let tag_start = script.span.start as usize;
            let source = ctx.source;
            let tag_len = script.span.end as usize - tag_start;
            let tag_text = &source[tag_start..tag_start + tag_len];
            for (offset, _) in tag_text.match_indices("$inspect") {
                if is_in_comment(tag_text, offset) {
                    continue;
                }
                let start = tag_start + offset;
                let end = start + "$inspect".len();
                ctx.diagnostic(
                    "Do not use $inspect directive",
                    oxc::span::Span::new(start as u32, end as u32),
                );
            }
        }
    }
}
