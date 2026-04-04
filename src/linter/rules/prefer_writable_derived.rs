//! `svelte/prefer-writable-derived` — prefer `$derived` with a setter over `$state` + `$effect`.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

fn is_single_assignment_effect(body: &str, assign_pattern: &str) -> bool {
    let bytes = body.as_bytes();
    let mut depth = 0i32;
    let mut stmt_count = 0;
    let mut first_stmt_start = None;
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
                if has_content { stmt_count += 1; }
                has_content = false;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                continue;
            }
            c if !c.is_ascii_whitespace() => {
                if !has_content && first_stmt_start.is_none() { first_stmt_start = Some(i); }
                has_content = true;
            }
            _ => {}
        }
        i += 1;
    }
    if has_content { stmt_count += 1; }
    if stmt_count != 1 { return false; }

    let stmt = body[first_stmt_start.unwrap_or(0)..].trim();
    if !stmt.starts_with(assign_pattern) { return false; }
    let after = &stmt[assign_pattern.len()..];
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

            let mut state_vars: Vec<(String, usize)> = Vec::new(); // (name, position)
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("let ") {
                    let state_pos = rest.find("$state(")
                        .or_else(|| rest.find("$state<").and_then(|p| {
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
                        let name_part = if let Some(colon) = name_part.find(':') {
                            name_part[..colon].trim()
                        } else {
                            name_part
                        };
                        if name_part.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            && !name_part.is_empty()
                        {
                            if let Some(pos) = content.find(trimmed) {
                                state_vars.push((name_part.to_string(), pos));
                            }
                        }
                    }
                }
            }

            for (var_name, var_pos) in &state_vars {
                let effect_pattern = format!("{} =", var_name);
                if !content.contains("$effect(") && !content.contains("$effect.pre(") { continue; }

                for needle in &["$effect(", "$effect.pre("] {
                    let mut search_from = 0;
                    while let Some(effect_pos) = content[search_from..].find(needle) {
                        let abs = search_from + effect_pos;
                        let rest = &content[abs..];
                        if let Some(body_start) = rest.find('{') {
                            let body = &rest[body_start..];
                            let mut depth = 0;
                            let mut body_end = body.len();
                            for (i, ch) in body.char_indices() {
                                match ch {
                                    '{' => depth += 1,
                                    '}' => { depth -= 1; if depth == 0 { body_end = i; break; } }
                                    _ => {}
                                }
                            }
                            if is_single_assignment_effect(&body[1..body_end], &effect_pattern) {
                                let sp = content_offset + var_pos;
                                ctx.diagnostic("Prefer using writable $derived instead of $state and $effect",
                                    oxc::span::Span::new(sp as u32, (sp + var_name.len() + 4) as u32));
                            }
                        }
                        search_from = abs + needle.len();
                    }
                }
            }
        }
    }
}
