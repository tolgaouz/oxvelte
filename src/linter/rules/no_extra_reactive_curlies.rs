//! `svelte/no-extra-reactive-curlies` — disallow unnecessary curly braces in reactive statements.
//! 💡
//!
//! Detects `$: { single_statement; }` patterns where the braces are unnecessary.

use crate::linter::{LintContext, Rule};

pub struct NoExtraReactiveCurlies;

impl Rule for NoExtraReactiveCurlies {
    fn name(&self) -> &'static str {
        "svelte/no-extra-reactive-curlies"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let tag_start = script.span.start as usize;
            let source = ctx.source;

            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find("$:") {
                let abs_pos = search_from + pos;
                let after = content[abs_pos + 2..].trim_start();

                // Check for `$: { ... }` pattern with a single statement inside
                if after.starts_with('{') {
                    // Find the matching closing brace using depth-aware search
                    let close = {
                        let mut depth = 0i32;
                        let mut found = None;
                        for (i, ch) in after.char_indices() {
                            match ch {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 { found = Some(i); break; }
                                }
                                _ => {}
                            }
                        }
                        found
                    };
                    if let Some(close) = close {
                        let block_body = after[1..close].trim();
                        // If the block body contains no semicolons (single statement)
                        // or exactly one semicolon at the end, it's unnecessary braces
                        let semicolons = block_body.matches(';').count();
                        if semicolons <= 1 {
                            let tag_text = &source[tag_start..script.span.end as usize];
                            if let Some(gt) = tag_text.find('>') {
                                // Offset to the `{` character: abs_pos + 2 (for "$:") + whitespace
                                let after_dollar_colon = &content[abs_pos + 2..];
                                let ws_len = after_dollar_colon.len() - after_dollar_colon.trim_start().len();
                                let open_brace_pos = abs_pos + 2 + ws_len;
                                let source_pos = tag_start + gt + 1 + open_brace_pos;
                                ctx.diagnostic(
                                    "Do not wrap reactive statements in curly braces unless necessary.",
                                    oxc::span::Span::new(source_pos as u32, (source_pos + 1) as u32),
                                );
                            }
                        }
                    }
                }
                search_from = abs_pos + 2;
            }
        }
    }
}
