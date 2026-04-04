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

        let check_async = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("checkAsyncFunctions"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

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

fn extract_function_props(content: &str, check_async: bool) -> Vec<(String, usize)> {
    let mut props = Vec::new();

    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
            let q = bytes[i]; i += 1;
            while i < bytes.len() && bytes[i] != q {
                if bytes[i] == b'\\' { i += 1; }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }

        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
            }
            let name = &content[start..i];

            if matches!(name, "interface" | "type" | "let" | "const" | "var" | "function" |
                "import" | "export" | "from" | "return" | "if" | "else" | "void" |
                "Props" | "string" | "number" | "boolean" | "any" | "never" | "unknown") {
                continue;
            }

            let rest = &content[i..].trim_start();
            let is_void_method = (rest.starts_with("():") || rest.starts_with("()")) && {
                let ap = if rest.starts_with("():") { &rest[3..] } else { &rest[2..] };
                let ap = ap.trim_start().trim_start_matches(':').trim_start();
                ap.starts_with("void") || (check_async && ap.starts_with("Promise<void>"))
            };
            let is_fn_type = rest.starts_with(':') && {
                let after_colon = rest[1..].trim_start();
                if after_colon.starts_with("()") || after_colon.starts_with("(e") || after_colon.starts_with("(arg") {
                    let has_void = after_colon.contains("=> void");
                    let has_promise_void = check_async && after_colon.contains("=> Promise<void>");
                    has_void || has_promise_void
                } else { false }
            };

            let is_any = rest.starts_with(": any") || rest.starts_with(":any");

            if (is_void_method || is_fn_type) && !is_any {
                let before = &content[..start];
                let brace_depth: i32 = before.bytes()
                    .fold(0i32, |d, b| match b { b'{' => d + 1, b'}' => d - 1, _ => d });
                if brace_depth > 0 {
                    let last_interface = before.rfind("interface ");
                    let last_type = before.rfind("type ");
                    let last_inline = before.rfind(": {");
                    let last_ctx = [last_interface, last_type, last_inline].iter()
                        .filter_map(|x| *x).max();
                    if let Some(ctx_pos) = last_ctx {
                        if let Some(open_brace) = content[ctx_pos..start].rfind('{') {
                            let brace_abs = ctx_pos + open_brace;
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
