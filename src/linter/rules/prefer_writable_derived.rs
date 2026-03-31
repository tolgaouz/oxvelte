//! `svelte/prefer-writable-derived` — prefer `$derived` with a setter over `$state` + `$effect`.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

/// Check if the effect body contains exactly one statement that is `varName = expr;`
fn is_single_assignment_effect(body: &str, assign_pattern: &str) -> bool {
    // Count top-level statements by finding `;` at brace/paren depth 0
    let bytes = body.as_bytes();
    let mut depth = 0i32;
    let mut stmt_count = 0;
    let mut first_stmt_start = None;
    let mut first_stmt_end = None;
    let mut in_str = false;
    let mut str_ch = 0u8;
    let mut i = 0;
    let mut has_content = false;

    while i < bytes.len() {
        if in_str {
            if bytes[i] == b'\\' { i += 2; continue; }
            if bytes[i] == str_ch { in_str = false; }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'\'' | b'"' | b'`' => { in_str = true; str_ch = bytes[i]; has_content = true; }
            b'{' | b'(' | b'[' => { depth += 1; has_content = true; }
            b'}' | b')' | b']' => { depth -= 1; }
            b';' if depth == 0 => {
                if has_content {
                    stmt_count += 1;
                    if stmt_count == 1 {
                        first_stmt_end = Some(i);
                    }
                }
                has_content = false;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                continue;
            }
            c if !c.is_ascii_whitespace() => {
                if !has_content && first_stmt_start.is_none() {
                    first_stmt_start = Some(i);
                }
                has_content = true;
            }
            _ => {}
        }
        i += 1;
    }
    // Count trailing content without semicolon as a statement
    if has_content {
        stmt_count += 1;
        if stmt_count == 1 {
            first_stmt_end = Some(bytes.len());
        }
    }

    if stmt_count != 1 { return false; }

    // Check that the single statement starts with `varName =` (not `varName ==`)
    let start = first_stmt_start.unwrap_or(0);
    let stmt = body[start..].trim();
    if !stmt.starts_with(assign_pattern) { return false; }
    let after = &stmt[assign_pattern.len()..];
    // Must not be `==` (comparison) or `=>` (arrow)
    !after.starts_with('=') && !after.starts_with('>')
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
                        // Strip type annotation: `varName: Type` → `varName`
                        let name_part = if let Some(colon) = name_part.find(':') {
                            name_part[..colon].trim()
                        } else {
                            name_part
                        };
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
