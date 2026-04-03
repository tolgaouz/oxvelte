//! `svelte/no-inner-declarations` — disallow function declarations in nested blocks.
//! ⭐ Recommended (Extension Rule)

use crate::linter::{LintContext, Rule};

pub struct NoInnerDeclarations;

impl Rule for NoInnerDeclarations {
    fn name(&self) -> &'static str {
        "svelte/no-inner-declarations"
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

            check_inner_declarations(content, content_offset, ctx);
        }
    }
}

/// Skip over a template literal body (after the opening backtick).
/// Handles nested template literals inside `${...}` expressions.
fn skip_template_literal(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() {
        if bytes[*i] == b'\\' {
            *i += 2;
            continue;
        }
        if bytes[*i] == b'`' {
            *i += 1; // consume closing backtick
            return;
        }
        if bytes[*i] == b'$' && *i + 1 < bytes.len() && bytes[*i + 1] == b'{' {
            *i += 2; // skip ${
            // Scan the expression inside ${...}, handling nested braces and template literals
            let mut depth = 1i32;
            while *i < bytes.len() && depth > 0 {
                if bytes[*i] == b'\\' {
                    *i += 1;
                } else if bytes[*i] == b'{' {
                    depth += 1;
                } else if bytes[*i] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        *i += 1; // consume closing }
                        break;
                    }
                } else if bytes[*i] == b'`' {
                    *i += 1; // skip opening backtick
                    skip_template_literal(bytes, i); // recurse for nested template
                    continue;
                } else if bytes[*i] == b'\'' || bytes[*i] == b'"' {
                    let q = bytes[*i];
                    *i += 1;
                    while *i < bytes.len() {
                        if bytes[*i] == b'\\' { *i += 1; }
                        else if bytes[*i] == q { break; }
                        *i += 1;
                    }
                } else if *i + 1 < bytes.len() && bytes[*i] == b'/' && bytes[*i + 1] == b'/' {
                    while *i < bytes.len() && bytes[*i] != b'\n' { *i += 1; }
                    continue;
                } else if *i + 1 < bytes.len() && bytes[*i] == b'/' && bytes[*i + 1] == b'*' {
                    *i += 2;
                    while *i + 1 < bytes.len() && !(bytes[*i] == b'*' && bytes[*i + 1] == b'/') { *i += 1; }
                    if *i + 1 < bytes.len() { *i += 2; }
                    continue;
                }
                *i += 1;
            }
            continue;
        }
        *i += 1;
    }
}

fn check_inner_declarations(content: &str, content_offset: usize, ctx: &mut LintContext<'_>) {
    let bytes = content.as_bytes();
    let mut i = 0;

    // Track scope: each entry is (depth_at_entry, is_control_flow)
    // Depth increases on `{`, decreases on `}`.
    // Control flow blocks: if, for, while, switch, else
    // Function bodies create new scopes where inner declarations are OK.
    let mut brace_depth = 0i32;
    let mut scope_stack: Vec<(i32, bool)> = Vec::new(); // (depth, is_function_body)

    while i < bytes.len() {
        // Skip single/double-quoted strings
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let q = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' { i += 1; }
                else if bytes[i] == q { break; }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }
        // Skip template literals (with proper nesting support)
        if bytes[i] == b'`' {
            i += 1;
            skip_template_literal(bytes, &mut i);
            continue;
        }
        // Skip line comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        // Skip block comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
            if i + 1 < bytes.len() { i += 2; }
            continue;
        }

        if bytes[i] == b'{' {
            // Detect arrow function body: `=> {`
            let before = content[..i].trim_end();
            if before.ends_with("=>") {
                scope_stack.push((brace_depth, true)); // arrow function body
            }
            brace_depth += 1;
            i += 1;
            continue;
        }
        if bytes[i] == b'}' {
            brace_depth -= 1;
            // Pop scope if we're leaving a tracked scope
            while let Some(&(depth, _)) = scope_stack.last() {
                if depth >= brace_depth {
                    scope_stack.pop();
                } else {
                    break;
                }
            }
            i += 1;
            continue;
        }

        // Skip non-ASCII bytes (multi-byte UTF-8 characters can't be JS keywords)
        if !bytes[i].is_ascii() {
            i += 1;
            continue;
        }

        let rest = &content[i..];

        // Detect control flow keywords that create blocks
        let kw_word_start = i == 0 || {
            let prev = bytes[i - 1];
            !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'$' && prev != b'.'
        };
        let is_control = kw_word_start && (rest.starts_with("if ") || rest.starts_with("if(")
            || rest.starts_with("else ") || rest.starts_with("else{")
            || rest.starts_with("for ") || rest.starts_with("for(")
            || rest.starts_with("while ") || rest.starts_with("while(")
            || rest.starts_with("switch ") || rest.starts_with("switch("));

        if is_control {
            scope_stack.push((brace_depth, false));
        }

        // Detect function keyword (creates new scope)
        if kw_word_start && (rest.starts_with("function ") || rest.starts_with("function(")) {
            // Check if this is a function DECLARATION (not expression)
            let before = content[..i].trim_end();
            let effective_before = if before.ends_with("async") {
                before[..before.len()-5].trim_end()
            } else {
                before
            };
            let is_expression = effective_before.ends_with('=')
                || effective_before.ends_with('(')
                || effective_before.ends_with(',')
                || effective_before.ends_with('!')
                || effective_before.ends_with("||")
                || effective_before.ends_with("&&")
                || effective_before.ends_with('?')
                || effective_before.ends_with(':')
                || effective_before.ends_with("return")
                || effective_before.ends_with("=>")
                || effective_before.is_empty();

            if !is_expression {
                // Only flag if inside control flow at the program/module level
                let in_control_flow = !scope_stack.is_empty()
                    && !scope_stack.iter().any(|&(_, is_fn)| is_fn);

                if in_control_flow && brace_depth > 0 {
                    let source_pos = content_offset + i;
                    let fn_rest = &content[i + 9..]; // skip "function "
                    let name_end = fn_rest.find(|c: char| !c.is_alphanumeric() && c != '_')
                        .unwrap_or(fn_rest.len());
                    let end_pos = source_pos + 9 + name_end;
                    ctx.diagnostic(
                        "Move function declaration to program root.",
                        oxc::span::Span::new(source_pos as u32, end_pos as u32),
                    );
                }
            }

            // Either way, the next `{` is a function body (new scope)
            scope_stack.push((brace_depth, true));
            // Skip past TS return type annotations that may contain `{}`
            let fn_start = i + 8; // skip "function"
            if let Some(paren_start) = content[fn_start..].find('(') {
                let mut j = fn_start + paren_start + 1;
                let mut pd = 1i32;
                while j < bytes.len() && pd > 0 {
                    match bytes[j] {
                        b'(' => pd += 1,
                        b')' => pd -= 1,
                        b'\'' | b'"' | b'`' => {
                            let q = bytes[j]; j += 1;
                            while j < bytes.len() && bytes[j] != q {
                                if bytes[j] == b'\\' { j += 1; }
                                j += 1;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let after_paren = content[j..].trim_start();
                if after_paren.starts_with(':') {
                    let type_start = j + (content[j..].len() - after_paren.len()) + 1;
                    let mut tj = type_start;
                    let mut ad = 0i32;
                    while tj < bytes.len() {
                        match bytes[tj] {
                            b'<' => ad += 1,
                            b'>' => { if ad > 0 { ad -= 1; } }
                            b'{' if ad == 0 => {
                                i = tj - 1;
                                break;
                            }
                            _ => {}
                        }
                        tj += 1;
                    }
                }
            }
        }

        i += 1;
    }
}
