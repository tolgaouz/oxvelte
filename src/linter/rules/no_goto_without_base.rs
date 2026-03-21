//! `svelte/no-goto-without-base` — require goto to use base path.

use crate::linter::{LintContext, Rule};

pub struct NoGotoWithoutBase;

impl Rule for NoGotoWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-goto-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            // Only check if goto is imported from $app/navigation
            if !content.contains("from '$app/navigation'") && !content.contains("from \"$app/navigation\"") {
                return;
            }
            if !content.contains("goto(") { return; }

            let has_base = content.contains("from '$app/paths'") || content.contains("from \"$app/paths\"")
                || content.contains("base");

            if !has_base {
                // Flag all goto calls with string literal arguments
                let base = script.span.start as usize;
                let source = ctx.source;
                let tag_text = &source[base..script.span.end as usize];
                let gt = tag_text.find('>').unwrap_or(0);

                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find("goto(") {
                    let abs = search_from + pos;
                    if abs > 0 {
                        let prev = content.as_bytes()[abs - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            search_from = abs + 5;
                            continue;
                        }
                    }
                    let rest = &content[abs + 5..];
                    let trimmed = rest.trim_start();
                    // Extract the string content to check if it's an absolute URI
                    let is_absolute_uri = if trimmed.starts_with('\'') || trimmed.starts_with('"') || trimmed.starts_with('`') {
                        let quote = trimmed.as_bytes()[0];
                        let inner = &trimmed[1..];
                        if let Some(end) = inner.find(quote as char) {
                            let s = &inner[..end];
                            s.starts_with("http://") || s.starts_with("https://")
                                || s.starts_with("mailto:") || s.starts_with("tel:")
                                || s.starts_with("//")
                        } else { false }
                    } else { false };
                    if !is_absolute_uri && (trimmed.starts_with('\'') || trimmed.starts_with('"') || trimmed.starts_with('`')) {
                        let source_pos = base + gt + 1 + abs;
                        ctx.diagnostic(
                            "Use `base` from `$app/paths` when calling `goto` with an absolute path.",
                            oxc::span::Span::new(source_pos as u32, (source_pos + 5) as u32),
                        );
                    }
                    search_from = abs + 5;
                }
            }
        }
    }
}
