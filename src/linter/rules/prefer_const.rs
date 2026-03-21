//! `svelte/prefer-const` — require `const` declarations for variables that are never reassigned.
//! 🔧 Fixable

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct PreferConst;

impl Rule for PreferConst {
    fn name(&self) -> &'static str {
        "svelte/prefer-const"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;

            // Simple heuristic: find `let` declarations and check if the variable
            // name appears on the left-hand side of an assignment later.
            for (offset, _) in content.match_indices("let ") {
                // Make sure it's at a statement boundary (start or preceded by whitespace/semicolon/newline).
                if offset > 0 {
                    let prev = content.as_bytes()[offset - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'_' {
                        continue;
                    }
                }
                // Extract the variable name after `let `.
                let rest = &content[offset + 4..];
                let var_end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
                if var_end == 0 {
                    continue;
                }
                let var_name = &rest[..var_end];
                // Skip declarations with Svelte runes ($derived, $state, $props, $bindable)
                let after_name = rest[var_end..].trim_start();
                if after_name.starts_with("= $derived") || after_name.starts_with("= $state")
                    || after_name.starts_with("= $props") || after_name.starts_with("= $bindable")
                {
                    continue;
                }
                // Also skip destructuring: let { ... } = $props()
                if var_name == "{" || var_name == "[" {
                    continue;
                }
                // Check if this variable is reassigned anywhere (simple: look for `var_name =` but not `==`).
                let after_decl = &content[offset + 4 + var_end..];
                let pattern = format!("{} =", var_name);
                let is_reassigned = after_decl.match_indices(&pattern).any(|(pos, _)| {
                    let after_eq = pos + pattern.len();
                    after_eq >= after_decl.len() || after_decl.as_bytes()[after_eq] != b'='
                });
                if !is_reassigned {
                    let start = (base + offset) as u32;
                    let end = start + 3; // length of "let"
                    ctx.diagnostic(
                        format!("`{}` is never reassigned. Use `const` instead.", var_name),
                        Span::new(start, end),
                    );
                }
            }
        }
    }
}
