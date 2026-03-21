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
        // Skip strings
        if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
            let q = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' { i += 1; }
                else if bytes[i] == q { break; }
                else if q == b'`' && bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    brace_depth += 1;
                    i += 1;
                }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
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

        let rest = &content[i..];

        // Detect control flow keywords that create blocks
        let is_control = rest.starts_with("if ") || rest.starts_with("if(")
            || rest.starts_with("else ") || rest.starts_with("else{")
            || rest.starts_with("for ") || rest.starts_with("for(")
            || rest.starts_with("while ") || rest.starts_with("while(")
            || rest.starts_with("switch ") || rest.starts_with("switch(");

        if is_control {
            scope_stack.push((brace_depth, false));
        }

        // Detect function keyword (creates new scope)
        if rest.starts_with("function ") || rest.starts_with("function(") {
            let is_word_start = i == 0 || {
                let prev = bytes[i - 1];
                !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'$'
            };

            if is_word_start {
                // Check if this is a function DECLARATION (not expression)
                // A function expression is preceded by =, (, ,, !, ||, &&, ?, :, return, etc.
                let before = content[..i].trim_end();
                let is_expression = before.ends_with('=')
                    || before.ends_with('(')
                    || before.ends_with(',')
                    || before.ends_with('!')
                    || before.ends_with("||")
                    || before.ends_with("&&")
                    || before.ends_with('?')
                    || before.ends_with(':')
                    || before.ends_with("return")
                    || before.ends_with("=>")
                    || before.is_empty();

                if !is_expression {
                    // This is a function declaration
                    // Check if we're inside a control flow block
                    let in_control_flow = scope_stack.iter().any(|(_, is_fn)| !is_fn);

                    if in_control_flow && brace_depth > 0 {
                        let source_pos = content_offset + i;
                        // Find end of function name
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
            }
        }

        i += 1;
    }
}
