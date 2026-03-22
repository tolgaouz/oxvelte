//! `svelte/require-event-prefix` — require event handler props to use the `on` prefix.

use crate::linter::{LintContext, Rule};

pub struct RequireEventPrefix;

impl Rule for RequireEventPrefix {
    fn name(&self) -> &'static str {
        "svelte/require-event-prefix"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let script = match &ctx.ast.instance { Some(s) => s, None => return };
        if script.lang.as_deref() != Some("ts") { return; }
        let content = &script.content;
        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        // Config: { "checkAsyncFunctions": true } — also check async (Promise<void>) function props
        let check_async = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("checkAsyncFunctions"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Find function-typed properties in Props type definitions
        // Patterns:
        // 1. interface Props { name(): void; }
        // 2. interface Props { name: () => void; }
        // 3. type Props = { name(): void; }
        // 4. Inline: let { name }: { name(): void } = $props();
        let fn_props = extract_function_props(content, check_async);

        for (prop_name, prop_offset) in &fn_props {
            if !prop_name.starts_with("on") {
                let src_pos = content_offset + prop_offset;
                ctx.diagnostic(
                    "Component event name must start with \"on\".",
                    oxc::span::Span::new(src_pos as u32, (src_pos + prop_name.len()) as u32),
                );
            }
        }
    }
}

/// Extract function-typed property names from TS type definitions.
/// Returns vec of (property_name, byte_offset_in_content).
fn extract_function_props(content: &str, check_async: bool) -> Vec<(String, usize)> {
    let mut props = Vec::new();

    // Find interface/type blocks and inline types
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip strings
        if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
            let q = bytes[i]; i += 1;
            while i < bytes.len() && bytes[i] != q {
                if bytes[i] == b'\\' { i += 1; }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }

        // Look for type property definitions inside { }
        // Pattern: identifier followed by () or : () =>
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
            }
            let name = &content[start..i];

            // Skip keywords
            if matches!(name, "interface" | "type" | "let" | "const" | "var" | "function" |
                "import" | "export" | "from" | "return" | "if" | "else" | "void" |
                "Props" | "string" | "number" | "boolean" | "any" | "never" | "unknown") {
                continue;
            }

            // Check what follows the identifier
            let rest = &content[i..].trim_start();
            // Check if this is a void-returning function property
            let is_void_method = {
                if rest.starts_with("():") || rest.starts_with("()") {
                    // name(): RETURN_TYPE — check return type is void
                    let after_parens = if rest.starts_with("():") { &rest[3..] } else { &rest[2..] };
                    let after_parens = after_parens.trim_start().trim_start_matches(':').trim_start();
                    let is_void_ret = after_parens.starts_with("void") && !after_parens.starts_with("void;")
                        || after_parens.starts_with("void;") || after_parens.starts_with("void\n");
                    let is_promise_void_ret = check_async && (
                        after_parens.starts_with("Promise<void>")
                    );
                    is_void_ret || is_promise_void_ret
                } else { false }
            };
            let is_fn_type = rest.starts_with(':') && {
                let after_colon = rest[1..].trim_start();
                // : () => void or : (e: Event) => void
                if after_colon.starts_with("()") || after_colon.starts_with("(e") || after_colon.starts_with("(arg") {
                    // Check return type contains void or Promise<void>
                    let has_void = after_colon.contains("=> void");
                    let has_promise_void = check_async && after_colon.contains("=> Promise<void>");
                    has_void || has_promise_void
                } else { false }
            };

            // Skip `any` type
            let is_any = rest.starts_with(": any") || rest.starts_with(":any");

            if (is_void_method || is_fn_type) && !is_any {
                // Must be inside a type/interface block — check brace depth
                // Count { and } before this position to determine if we're inside a type block
                let before = &content[..start];
                let brace_depth: i32 = before.bytes()
                    .fold(0i32, |d, b| match b { b'{' => d + 1, b'}' => d - 1, _ => d });
                // Must be at depth > 0 (inside braces) AND preceded by type/interface context
                if brace_depth > 0 {
                    // Find the last type/interface/inline-type declaration
                    let last_interface = before.rfind("interface ");
                    let last_type = before.rfind("type ");
                    let last_inline = before.rfind(": {");
                    let last_ctx = [last_interface, last_type, last_inline].iter()
                        .filter_map(|x| *x).max();
                    if let Some(ctx_pos) = last_ctx {
                        // Check that the { after the type keyword encompasses our position
                        if let Some(open_brace) = content[ctx_pos..start].rfind('{') {
                            let brace_abs = ctx_pos + open_brace;
                            // Find matching close brace
                            let mut d = 0i32;
                            let mut found_close = false;
                            for (j, b) in content[brace_abs..].bytes().enumerate() {
                                match b {
                                    b'{' => d += 1,
                                    b'}' => { d -= 1; if d == 0 { found_close = brace_abs + j >= start; break; } }
                                    _ => {}
                                }
                            }
                            if found_close {
                                props.push((name.to_string(), start));
                            }
                        }
                    }
                }
            }
            continue;
        }
        i += 1;
    }
    props
}
