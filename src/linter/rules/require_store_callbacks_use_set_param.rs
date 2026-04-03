//! `svelte/require-store-callbacks-use-set-param` — require that store callbacks
//! use the `set` parameter provided by the callback.
//! 💡 Has suggestion

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct RequireStoreCallbacksUseSetParam;

impl Rule for RequireStoreCallbacksUseSetParam {
    fn name(&self) -> &'static str {
        "svelte/require-store-callbacks-use-set-param"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;

        for factory in &["readable(", "writable("] {
            for (offset, _) in content.match_indices(factory) {
                if offset > 0 && { let p = content.as_bytes()[offset - 1]; p.is_ascii_alphanumeric() || p == b'_' } { continue; }
                let rest = &content[offset..];
                let (mut depth, mut found_comma, mut cb_start) = (0i32, false, 0);
                for (i, ch) in rest.char_indices() {
                    match ch {
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' | '}' => { depth -= 1; if depth == 0 { break; } }
                        ',' if depth == 1 && !found_comma => { found_comma = true; cb_start = i + 1; }
                        _ => {}
                    }
                }
                if !found_comma { continue; }
                let cb = rest[cb_start..].trim_start();

                let has_set_param = |params: &str| params.split(',').any(|p| p.trim() == "set");
                let has_set = if cb.starts_with("function") {
                    cb.find('(').and_then(|ps| cb[ps..].find(')').map(|pe| has_set_param(&cb[ps+1..ps+pe]))).unwrap_or(false)
                } else if let Some(ap) = cb.find("=>") {
                    let b = cb[..ap].trim();
                    if b.starts_with('(') && b.ends_with(')') { has_set_param(&b[1..b.len()-1]) } else { b == "set" }
                } else { continue; };

                if !has_set {
                    let start = (base + offset) as u32;
                    ctx.diagnostic("Store callbacks must use `set` param.", Span::new(start, start + factory.len() as u32));
                }
            }
        }
    }
}
