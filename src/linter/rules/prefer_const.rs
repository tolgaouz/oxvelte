//! `svelte/prefer-const` — require `const` declarations for variables that are never reassigned.
//! 🔧 Fixable

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct PreferConst;

/// Extract rune name from an initializer expression (e.g. "$state(0)" -> "$state")
fn extract_rune_name(init: &str) -> Option<&str> {
    if init.starts_with('$') {
        // Find end of rune name (up to '(' or '.')
        let end = init.find(|c: char| c == '(' || c == '.').unwrap_or(init.len());
        let name = &init[..end];
        if !name.is_empty() && name[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Some(name);
        }
    }
    None
}

impl Rule for PreferConst {
    fn name(&self) -> &'static str {
        "svelte/prefer-const"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Parse excludedRunes from config. Default: all Svelte runes excluded.
        let default_excluded = vec!["$state", "$derived", "$props", "$bindable"];
        let excluded_runes: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|o| o.get("excludedRunes"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| default_excluded.iter().map(|s| s.to_string()).collect());

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
                    // Could be destructuring: let { prop1, prop2 } = $props()
                    if rest.starts_with('{') || rest.starts_with('[') {
                        // Find the closing brace/bracket
                        let close_char = if rest.starts_with('{') { '}' } else { ']' };
                        if let Some(close_pos) = rest.find(close_char) {
                            let after_destructure = rest[close_pos + 1..].trim_start();
                            // Check if this is a rune assignment
                            if after_destructure.starts_with("= ") || after_destructure.starts_with("=\t") {
                                let init = after_destructure[1..].trim_start();
                                let rune_name = extract_rune_name(init);
                                if let Some(rune) = rune_name {
                                    if excluded_runes.iter().any(|r| r == rune) {
                                        continue;
                                    }
                                }
                            }
                            // Extract individual variable names from destructuring
                            let inner = &rest[1..close_pos];
                            for part in inner.split(',') {
                                let var = part.trim();
                                // Handle renaming: original: renamed
                                let var = if let Some((_orig, renamed)) = var.split_once(':') {
                                    renamed.trim()
                                } else {
                                    var
                                };
                                // Handle default values: name = default
                                let var = var.split('=').next().unwrap_or(var).trim();
                                // Handle rest: ...rest
                                let var = var.strip_prefix("...").unwrap_or(var);
                                if var.is_empty() || !var.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                                    continue;
                                }
                                // Check if this variable is reassigned
                                let after_decl = &content[offset + 4 + close_pos + 1..];
                                let pattern = format!("{} =", var);
                                let is_reassigned = after_decl.match_indices(&pattern).any(|(pos, _)| {
                                    let after_eq = pos + pattern.len();
                                    after_eq >= after_decl.len() || after_decl.as_bytes()[after_eq] != b'='
                                });
                                if !is_reassigned {
                                    // Find the position of this variable name in the source
                                    let var_in_inner = inner.find(var).unwrap_or(0);
                                    let var_abs_offset = base + offset + 4 + 1 + var_in_inner;
                                    ctx.diagnostic(
                                        format!("'{}' is never reassigned. Use 'const' instead.", var),
                                        Span::new(var_abs_offset as u32, (var_abs_offset + var.len()) as u32),
                                    );
                                }
                            }
                        }
                    }
                    continue;
                }
                let var_name = &rest[..var_end];
                // Check if initializer uses a Svelte rune
                let after_name = rest[var_end..].trim_start();
                if after_name.starts_with("= ") || after_name.starts_with("=\t") || after_name.starts_with("=$") {
                    let init_start = if after_name.starts_with("=$") { 1 } else { 1 };
                    let init = after_name[init_start..].trim_start();
                    let rune_name = extract_rune_name(init);
                    if let Some(rune) = rune_name {
                        if excluded_runes.iter().any(|r| r == rune) {
                            continue;
                        }
                    }
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
