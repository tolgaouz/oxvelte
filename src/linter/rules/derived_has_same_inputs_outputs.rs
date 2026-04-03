//! `svelte/derived-has-same-inputs-outputs` — require `$derived` stores to use the
//! same names for inputs and outputs.
//! 💡 Has suggestion

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct DerivedHasSameInputsOutputs;

impl Rule for DerivedHasSameInputsOutputs {
    fn name(&self) -> &'static str {
        "svelte/derived-has-same-inputs-outputs"
    }

    fn applies_to_scripts(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;
        for (offset, _) in content.match_indices("derived(") {
            if offset > 0 && { let p = content.as_bytes()[offset - 1]; p.is_ascii_alphanumeric() || p == b'_' } { continue; }
            let rest = &content[offset + 8..];
            let Some(comma) = rest.find(',') else { continue };
            let store = rest[..comma].trim();
            let after = rest[comma + 1..].trim_start();
            let param = if after.starts_with('(') { after[1..].split(')').next().map(|s| s.trim()) }
                else { after.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$').next() };
            if let Some(p) = param {
                let expected = format!("${}", store);
                if !p.is_empty() && p != expected && p != store {
                    let s = (base + offset) as u32;
                    ctx.diagnostic(format!("The argument name should be '{}'.", expected), Span::new(s, s + 8));
                }
            }
        }
    }
}
