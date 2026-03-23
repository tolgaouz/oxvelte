//! `svelte/no-trailing-spaces` — disallow trailing whitespace at the end of lines.
//! 🔧 Fixable (Extension Rule)

use crate::linter::{LintContext, Rule, Fix};
use oxc::span::Span;

pub struct NoTrailingSpaces;

impl Rule for NoTrailingSpaces {
    fn name(&self) -> &'static str {
        "svelte/no-trailing-spaces"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Find script regions to skip (the rule only checks template lines)
        let script_regions = find_script_regions(ctx.source);

        let mut offset = 0usize;
        for (line_num, line) in ctx.source.lines().enumerate() {
            let line_start = offset;
            let line_end = offset + line.len();

            // Skip lines inside <script> blocks
            let in_script = script_regions.iter().any(|(s, e)| line_start >= *s && line_end <= *e);

            if !in_script {
                let trimmed = line.trim_end();
                if trimmed.len() < line.len() {
                    let trailing_start = line_start + trimmed.len();
                    let trailing_end = line_end;
                    ctx.diagnostic_with_fix(
                        "Trailing spaces not allowed.",
                        Span::new(trailing_start as u32, trailing_end as u32),
                        Fix {
                            span: Span::new(trailing_start as u32, trailing_end as u32),
                            replacement: String::new(),
                        },
                    );
                }
            }

            offset = line_end + 1; // +1 for newline
            let _ = line_num;
        }
    }
}

/// Find (start, end) byte ranges of <script>...</script> blocks.
fn find_script_regions(source: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut search_from = 0;
    while let Some(start) = source[search_from..].find("<script") {
        let abs_start = search_from + start;
        if let Some(close) = source[abs_start..].find("</script") {
            let abs_close = abs_start + close;
            if let Some(gt) = source[abs_close..].find('>') {
                let abs_end = abs_close + gt + 1;
                regions.push((abs_start, abs_end));
                search_from = abs_end;
                continue;
            }
        }
        break;
    }
    regions
}
