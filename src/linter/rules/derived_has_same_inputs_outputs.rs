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
        // Look for `derived(` in scripts and check that the callback parameter
        // matches the store being derived from. This is a heuristic approach.
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            for (offset, _) in content.match_indices("derived(") {
                // Check the character before to make sure it's a word boundary.
                if offset > 0 {
                    let prev = content.as_bytes()[offset - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'_' {
                        continue;
                    }
                }
                let start = (base + offset) as u32;
                let end = start + "derived(".len() as u32;
                // Extract the argument list heuristically.
                let rest = &content[offset + "derived(".len()..];
                if let Some(comma_pos) = rest.find(',') {
                    let store_arg = rest[..comma_pos].trim();
                    // Find the callback parameter.
                    let after_comma = rest[comma_pos + 1..].trim_start();
                    // Look for ($param) => or param =>
                    let param = if after_comma.starts_with('(') {
                        after_comma[1..].split(')').next().map(|s| s.trim())
                    } else {
                        after_comma.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$').next()
                    };
                    if let Some(param) = param {
                        let expected = format!("${}", store_arg);
                        if !param.is_empty() && param != expected && param != store_arg {
                            ctx.diagnostic(
                                format!(
                                    "The argument name should be '{}'.",
                                    expected
                                ),
                                Span::new(start, end),
                            );
                        }
                    }
                }
            }
        }
    }
}
