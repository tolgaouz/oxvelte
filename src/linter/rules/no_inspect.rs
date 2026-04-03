//! `svelte/no-inspect` — disallow use of `$inspect`.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

fn is_in_comment(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    let (mut i, mut state) = (0usize, 0u8); // 0=normal, 1=line comment, 2=block comment, 3=string
    let mut str_ch = 0u8;
    while i < pos {
        match state {
            1 => { if bytes[i] == b'\n' { state = 0; } i += 1; }
            2 => { if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' { state = 0; i += 2; } else { i += 1; } }
            3 => { if bytes[i] == b'\\' { i += 2; continue; } if bytes[i] == str_ch { state = 0; } i += 1; }
            _ => match bytes[i] {
                b'\'' | b'"' | b'`' => { state = 3; str_ch = bytes[i]; i += 1; }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => { state = 1; i += 2; }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => { state = 2; i += 2; }
                _ => i += 1,
            }
        }
    }
    state == 1 || state == 2
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
            let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
            for (offset, _) in tag_text.match_indices("$inspect") {
                if is_in_comment(tag_text, offset) { continue; }
                let start = script.span.start as usize + offset;
                ctx.diagnostic("Do not use $inspect directive", oxc::span::Span::new(start as u32, (start + 8) as u32));
            }
        }
    }
}
