//! `svelte/prefer-writable-derived` — prefer `$derived` with a setter over `$state` + `$effect`.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

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
                    if let Some(state_pos) = rest.find("$state(") {
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

                        // Check if the effect body UNCONDITIONALLY reassigns our var
                        // at depth 1 (directly in effect body, not nested in callbacks like untrack)
                        let has_simple_reassign = {
                            let mut depth = 0i32;
                            let mut found = false;
                            for line in effect_body.lines() {
                                for ch in line.chars() {
                                    match ch {
                                        '{' | '(' => depth += 1,
                                        '}' | ')' => depth -= 1,
                                        _ => {}
                                    }
                                }
                                let t = line.trim();
                                if depth <= 0 && t.starts_with(&effect_pattern) && !t.starts_with("if") && !t.starts_with("for") {
                                    found = true;
                                }
                            }
                            found
                        };
                        // Also check it's not inside a conditional
                        let has_conditional = effect_body.contains("if ") || effect_body.contains("if(")
                            || effect_body.contains("for ") || effect_body.contains("while ");
                        if has_simple_reassign && !has_conditional {
                            let source_pos = content_offset + var_pos;
                            ctx.diagnostic(
                                "Prefer using writable $derived instead of $state and $effect",
                                oxc::span::Span::new(source_pos as u32, (source_pos + var_name.len() + 4) as u32),
                            );
                            break;
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

                        let has_simple_reassign2 = {
                            let mut depth = 0i32;
                            let mut found = false;
                            for line in effect_body.lines() {
                                for ch in line.chars() {
                                    match ch {
                                        '{' | '(' => depth += 1,
                                        '}' | ')' => depth -= 1,
                                        _ => {}
                                    }
                                }
                                let t = line.trim();
                                if depth <= 0 && t.starts_with(&effect_pattern) && !t.starts_with("if") && !t.starts_with("for") {
                                    found = true;
                                }
                            }
                            found
                        };
                        let has_conditional2 = effect_body.contains("if ") || effect_body.contains("if(")
                            || effect_body.contains("for ") || effect_body.contains("while ");
                        if has_simple_reassign2 && !has_conditional2 {
                            let source_pos = content_offset + var_pos;
                            ctx.diagnostic(
                                "Prefer using writable $derived instead of $state and $effect",
                                oxc::span::Span::new(source_pos as u32, (source_pos + var_name.len() + 4) as u32),
                            );
                            break;
                        }
                    }

                    search_from = abs + 12;
                }
            }
        }
    }
}
