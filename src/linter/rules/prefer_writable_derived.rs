//! `svelte/prefer-writable-derived` — prefer `$derived` with a setter over `$state` + `$effect`.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

/// Check if the effect body contains exactly one statement that is `varName = expr;`
fn is_single_assignment_effect(body: &str, assign_pattern: &str) -> bool {
    // Collect non-empty, non-comment lines
    let stmts: Vec<&str> = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("/*"))
        .collect();

    // Must have exactly one logical statement
    // A single statement may span multiple lines (e.g., `foo =\n  bar;`)
    // Concatenate and split by `;` to count statements
    let joined: String = stmts.join(" ");
    let parts: Vec<&str> = joined.split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() != 1 {
        return false;
    }

    // The single statement must start with `varName =` (simple assignment)
    let stmt = parts[0].trim();
    stmt.starts_with(assign_pattern)
}

pub struct PreferWritableDerived;

impl Rule for PreferWritableDerived {
    fn name(&self) -> &'static str {
        "svelte/prefer-writable-derived"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

            // Find let name = $state(expr); patterns
            let mut state_vars: Vec<(String, usize)> = Vec::new(); // (name, position)
            for (line_offset, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("let ") {
                    // Match $state( or $state<...>(
                    let state_pos = rest.find("$state(")
                        .or_else(|| rest.find("$state<").and_then(|p| {
                            // Find matching > then (
                            let after = &rest[p + 7..]; // after "$state<"
                            let mut depth = 1i32;
                            for (i, ch) in after.char_indices() {
                                match ch {
                                    '<' => depth += 1,
                                    '>' => { depth -= 1; if depth == 0 {
                                        let rest_after = &after[i+1..];
                                        if rest_after.starts_with('(') { return Some(p); }
                                        return None;
                                    }}
                                    _ => {}
                                }
                            }
                            None
                        }));
                    if let Some(state_pos) = state_pos {
                        let name_part = rest[..state_pos].trim_end().trim_end_matches('=').trim();
                        if name_part.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            && !name_part.is_empty()
                        {
                            // Find the actual position in content
                            if let Some(pos) = content.find(trimmed) {
                                state_vars.push((name_part.to_string(), pos));
                            }
                        }
                    }
                }
            }

            // For each $state var, check if there's a $effect that reassigns it
            for (var_name, var_pos) in &state_vars {
                // Look for $effect(() => { var_name = ...; });
                let effect_pattern = format!("{} =", var_name);
                let has_any_effect = content.contains("$effect(") || content.contains("$effect.pre(");
                if !has_any_effect { continue; }

                // Check if $effect contains reassignment of this var
                let mut search_from = 0;
                while let Some(effect_pos) = content[search_from..].find("$effect(") {
                    let abs = search_from + effect_pos;
                    let rest = &content[abs..];

                    // Find the body of the effect (between { and })
                    if let Some(body_start) = rest.find('{') {
                        let body = &rest[body_start..];
                        let mut depth = 0;
                        let mut body_end = body.len();
                        for (i, ch) in body.char_indices() {
                            match ch {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 { body_end = i; break; }
                                }
                                _ => {}
                            }
                        }
                        let effect_body = &body[1..body_end];

                        // Check if the effect body contains EXACTLY ONE statement:
                        // a simple assignment `varName = expr;`
                        let is_single_assign = is_single_assignment_effect(effect_body, &effect_pattern);
                        if is_single_assign {
                            let source_pos = content_offset + var_pos;
                            ctx.diagnostic(
                                "Prefer using writable $derived instead of $state and $effect",
                                oxc::span::Span::new(source_pos as u32, (source_pos + var_name.len() + 4) as u32),
                            );
                        }
                    }

                    search_from = abs + 8;
                }

                // Also check $effect.pre(
                search_from = 0;
                while let Some(effect_pos) = content[search_from..].find("$effect.pre(") {
                    let abs = search_from + effect_pos;
                    let rest = &content[abs..];

                    if let Some(body_start) = rest.find('{') {
                        let body = &rest[body_start..];
                        let mut depth = 0;
                        let mut body_end = body.len();
                        for (i, ch) in body.char_indices() {
                            match ch {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 { body_end = i; break; }
                                }
                                _ => {}
                            }
                        }
                        let effect_body = &body[1..body_end];

                        let is_single_assign2 = is_single_assignment_effect(effect_body, &effect_pattern);
                        if is_single_assign2 {
                            let source_pos = content_offset + var_pos;
                            ctx.diagnostic(
                                "Prefer using writable $derived instead of $state and $effect",
                                oxc::span::Span::new(source_pos as u32, (source_pos + var_name.len() + 4) as u32),
                            );
                        }
                    }

                    search_from = abs + 12;
                }
            }
        }
    }
}
