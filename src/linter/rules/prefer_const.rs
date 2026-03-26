//! `svelte/prefer-const` — require `const` declarations for variables that are never reassigned.
//! 🔧 Fixable
//!
//! Mirrors the vendor eslint-plugin-svelte approach: wraps ESLint core `prefer-const`
//! logic with a Svelte-specific filter that skips declarations initialized with
//! excluded runes (`$props`, `$derived` by default). When a `VariableDeclaration`
//! contains any declarator whose init is an excluded rune call (direct or member
//! expression like `$derived.by(...)`), the entire declaration is skipped.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct PreferConst;

/// Given an initializer expression string (the RHS of `=`), extract the rune name
/// if the expression is a rune call like `$state(0)` or `$derived.by(calc())`.
///
/// - `$state(0)` → Some("$state")
/// - `$derived.by(fn)` → Some("$derived")
/// - `$props()` → Some("$props")
/// - `calc()` → None
/// - `0` → None
fn extract_rune_name(init: &str) -> Option<&str> {
    let init = init.trim();
    if !init.starts_with('$') {
        return None;
    }
    // Rune name ends at `(` (direct call) or `.` (member like $derived.by)
    let end = init.find(|c: char| c == '(' || c == '.').unwrap_or(init.len());
    let name = &init[..end];
    if name.len() > 1 && name[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(name)
    } else {
        None
    }
}

/// Check if `var_name` is reassigned anywhere in `source_after_decl`.
/// Looks for: `name =` (not `==`), `name +=`, `name++`, `++name`, etc.
fn is_var_reassigned(var_name: &str, source_after_decl: &str) -> bool {
    // Simple assignment: `name =` but not `name ==` or `name =>`
    let assign_pattern = format!("{} =", var_name);
    let has_simple_assign = source_after_decl.match_indices(&assign_pattern).any(|(pos, _)| {
        let after = pos + assign_pattern.len();
        if after >= source_after_decl.len() { return true; }
        let next = source_after_decl.as_bytes()[after];
        // Not == or =>
        next != b'=' && next != b'>'
    });
    if has_simple_assign { return true; }

    // Also check: name= (no space)
    let assign_nospace = format!("{}=", var_name);
    let has_nospace = source_after_decl.match_indices(&assign_nospace).any(|(pos, _)| {
        // Word boundary before
        if pos > 0 {
            let prev = source_after_decl.as_bytes()[pos - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' { return false; }
        }
        let after = pos + assign_nospace.len();
        if after >= source_after_decl.len() { return true; }
        let next = source_after_decl.as_bytes()[after];
        // Must not be == or => or part of ===
        next != b'=' && next != b'>'
    });
    if has_nospace { return true; }

    // Compound assignments and increment/decrement
    for op in &["++", "--"] {
        // postfix: name++
        if source_after_decl.contains(&format!("{}{}", var_name, op)) { return true; }
        // prefix: ++name
        if source_after_decl.contains(&format!("{}{}", op, var_name)) { return true; }
    }
    for op in &["+=", "-=", "*=", "/=", "%=", "|=", "&=", "^=", "<<=", ">>=", ">>>=", "**=", "&&=", "||=", "??="] {
        if source_after_decl.contains(&format!("{} {}", var_name, op))
            || source_after_decl.contains(&format!("{}{}", var_name, op)) {
            return true;
        }
    }

    false
}

impl Rule for PreferConst {
    fn name(&self) -> &'static str {
        "svelte/prefer-const"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Parse excludedRunes from config. Vendor default: ['$props', '$derived']
        let excluded_runes: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|o| o.get("excludedRunes"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["$props".into(), "$derived".into()]);

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);
            let content_start = script.span.start as usize + gt + 1;

            // Process each line to find `let` declarations
            for (offset, _) in content.match_indices("let ") {
                // Word boundary check
                if offset > 0 {
                    let prev = content.as_bytes()[offset - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' {
                        continue;
                    }
                }

                let rest = &content[offset + 4..];

                // --- Destructuring: let { ... } = expr  or  let [ ... ] = expr ---
                if rest.starts_with('{') || rest.starts_with('[') {
                    let close_char = if rest.starts_with('{') { '}' } else { ']' };
                    let Some(close_pos) = rest.find(close_char) else { continue; };
                    let after_close = rest[close_pos + 1..].trim_start();

                    // Must have an initializer
                    if !after_close.starts_with('=') { continue; }
                    let init = after_close[1..].trim_start();

                    // Vendor logic: if ANY declarator in this declaration has an
                    // excluded rune init, skip the ENTIRE declaration.
                    if let Some(rune) = extract_rune_name(init) {
                        if excluded_runes.iter().any(|r| r == rune) {
                            continue;
                        }
                    }

                    // Check each destructured variable
                    let inner = &rest[1..close_pos];
                    let after_decl = &content[offset + 4 + close_pos + 1..];
                    let mut char_pos = 0;
                    for part in inner.split(',') {
                        let trimmed = part.trim();
                        if !trimmed.is_empty() {
                            // Handle rename: `key: localName`
                            let local = if let Some((_, renamed)) = trimmed.split_once(':') {
                                renamed.trim()
                            } else {
                                trimmed
                            };
                            // Handle default: `name = default`
                            let local = local.split('=').next().unwrap_or(local).trim();
                            // Handle rest: `...rest`
                            let local = local.strip_prefix("...").unwrap_or(local);

                            if !local.is_empty()
                                && local.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                                && !is_var_reassigned(local, after_decl)
                            {
                                // Report at the variable name position
                                let name_offset_in_inner = inner[char_pos..].find(local)
                                    .map(|p| char_pos + p)
                                    .unwrap_or(char_pos);
                                let abs = content_start + offset + 4 + 1 + name_offset_in_inner;
                                ctx.diagnostic(
                                    format!("'{}' is never reassigned. Use 'const' instead.", local),
                                    Span::new(abs as u32, (abs + local.len()) as u32),
                                );
                            }
                        }
                        char_pos += part.len() + 1; // +1 for comma
                    }
                    continue;
                }

                // --- Simple declaration: let name = expr ---
                let var_end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
                    .unwrap_or(rest.len());
                if var_end == 0 { continue; }

                let var_name = &rest[..var_end];
                let mut after_name = rest[var_end..].trim_start();

                // Skip TypeScript type annotation: `let foo: Type = ...`
                if after_name.starts_with(':') {
                    // Find the `=` after the type annotation, handling generics like `Type<A, B>`
                    let mut depth = 0i32;
                    let mut found_eq = false;
                    for (i, ch) in after_name.char_indices() {
                        match ch {
                            '<' | '(' => depth += 1,
                            '>' | ')' => { if depth > 0 { depth -= 1; } }
                            '=' if depth == 0 && i > 0 => {
                                // Make sure it's not part of => or ==
                                let next = after_name.as_bytes().get(i + 1).copied().unwrap_or(0);
                                if next != b'>' && next != b'=' {
                                    after_name = &after_name[i..];
                                    found_eq = true;
                                    break;
                                }
                            }
                            ';' | '\n' if depth == 0 => break, // end of statement, no initializer
                            _ => {}
                        }
                    }
                    if !found_eq {
                        continue; // No initializer found after type annotation
                    }
                }

                // Skip uninitialized: `let foo;`
                if !after_name.starts_with('=') {
                    continue;
                }

                // Check if initializer is an excluded rune
                let init = after_name[1..].trim_start();
                if let Some(rune) = extract_rune_name(init) {
                    if excluded_runes.iter().any(|r| r == rune) {
                        continue;
                    }
                }

                // Check reassignment
                let after_decl = &content[offset + 4 + var_end..];
                if !is_var_reassigned(var_name, after_decl) {
                    let abs = content_start + offset + 4; // position of variable name
                    ctx.diagnostic(
                        format!("'{}' is never reassigned. Use 'const' instead.", var_name),
                        Span::new(abs as u32, (abs + var_name.len()) as u32),
                    );
                }
            }
        }
    }
}
