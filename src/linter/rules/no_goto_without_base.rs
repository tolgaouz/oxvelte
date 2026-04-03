//! `svelte/no-goto-without-base` — require goto to use base path.

use crate::linter::{parse_imports, LintContext, Rule};

pub struct NoGotoWithoutBase;

impl Rule for NoGotoWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-goto-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let imports = parse_imports(content);
        let resolve = |local: &str, imported: &str, suffix: &str| {
            if imported == "*" { format!("{}.{}", local, suffix) } else { local.to_string() }
        };
        let goto_names: Vec<String> = imports.iter()
            .filter(|(_, imp, m)| (imp == "goto" || imp == "*") && m == "$app/navigation")
            .map(|(l, imp, _)| resolve(l, imp, "goto")).collect();
        if goto_names.is_empty() { return; }

        let base_local: Option<String> = imports.iter()
            .find(|(_, imp, m)| (imp == "base" || imp == "*") && m == "$app/paths")
            .map(|(l, imp, _)| resolve(l, imp, "base"));

        let base = script.span.start as usize;
        let gt = ctx.source[base..script.span.end as usize].find('>').unwrap_or(0);

        for goto_name in &goto_names {
            let pat = format!("{}(", goto_name);
            let mut search_from = 0;
            while let Some(pos) = content[search_from..].find(&pat) {
                let abs = search_from + pos;
                if abs > 0 && { let p = content.as_bytes()[abs - 1]; p.is_ascii_alphanumeric() || p == b'_' } {
                    search_from = abs + pat.len(); continue;
                }
                let trimmed = content[abs + pat.len()..].trim_start();
                if matches!(trimmed.as_bytes().first(), Some(b'\'' | b'"' | b'`')) {
                    let quote = trimmed.as_bytes()[0];
                    let inner = &trimmed[1..];
                    let is_abs_uri = inner.find(quote as char).map_or(false, |end| {
                        let s = &inner[..end];
                        s.starts_with("http://") || s.starts_with("https://") || s.starts_with("mailto:") || s.starts_with("tel:") || s.starts_with("//")
                    });
                    if !is_abs_uri {
                        let call_body = &content[abs..abs + content[abs..].find(')').unwrap_or(content.len() - abs)];
                        let uses_base = base_local.as_ref().map_or(false, |bn| {
                            call_body.contains(&format!("`${{{}}}", bn)) || call_body.contains(&format!("{} +", bn)) || call_body.contains(&format!("{}+", bn))
                        });
                        if !uses_base {
                            let sp = base + gt + 1 + abs;
                            ctx.diagnostic("Use `base` from `$app/paths` when calling `goto` with an absolute path.",
                                oxc::span::Span::new(sp as u32, (sp + pat.len()) as u32));
                        }
                    }
                }
                search_from = abs + pat.len();
            }
        }
    }
}
