//! `svelte/indent` — enforce consistent indentation (2 spaces default).
//! 🔧 Fixable
//!
//! Checks template content indentation. Skips script/style blocks,
//! multiline tag attributes, and prettier-ignore sections.

use crate::linter::{LintContext, Rule};

const INDENT: usize = 2;

pub struct Indent;

impl Rule for Indent {
    fn name(&self) -> &'static str {
        "svelte/indent"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let source = ctx.source;
        let lines: Vec<&str> = source.lines().collect();
        let mut offset = 0usize;
        let mut skip_tag: Option<&str> = None;
        let mut skip_next_line = false; // prettier-ignore: skip just the next non-empty line
        let mut depth = 0i32;
        let mut in_multiline_tag = false;
        let mut multiline_tag_depth = 0i32;
        let mut multiline_tag_ignored = false;
        let mut multiline_tag_column = 0usize; // column of the opening <

        for &line in &lines {
            let line_start = offset;
            offset += line.len() + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }

            // Skip style/template blocks (script is checked for indentation)
            if let Some(end) = skip_tag {
                if trimmed.starts_with(end) { skip_tag = None; }
                continue;
            }
            if trimmed.starts_with("<style") { skip_tag = Some("</style"); continue; }
            if trimmed.starts_with("<template") && trimmed.contains("lang=") {
                skip_tag = Some("</template");
                continue;
            }
            // Script tags: just skip the tag line itself, not content
            if trimmed.starts_with("<script") || trimmed.starts_with("</script") {
                // Track depth for script open/close
                if trimmed.starts_with("<script") && !trimmed.ends_with("/>") {
                    depth += 1;
                }
                if trimmed.starts_with("</script") {
                    depth -= 1;
                    if depth < 0 { depth = 0; }
                }
                continue;
            }

            // prettier-ignore: skip just the next line's indentation check
            if trimmed.contains("prettier-ignore") {
                skip_next_line = true;
                continue;
            }

            // Multiline tag: attribute lines are not checked (they use column-relative
            // indentation which requires tracking the opening tag's column position)
            if in_multiline_tag {
                let is_end = trimmed.ends_with(">") || trimmed.ends_with("/>") || trimmed == ">" || trimmed == "/";
                if is_end {
                    in_multiline_tag = false;
                    multiline_tag_ignored = false;
                    if !trimmed.ends_with("/>") && trimmed != "/>" {
                        depth += 1;
                    }
                }
                continue;
            }

            let opens = count_opens(trimmed);
            let closes = count_closes(trimmed);

            // Skip check for this line (prettier-ignore)
            if skip_next_line {
                skip_next_line = false;
                if trimmed.starts_with('<') && !trimmed.starts_with("</") && !trimmed.starts_with("<!--") && !trimmed.contains('>') {
                    in_multiline_tag = true;
                    multiline_tag_depth = depth;
                    multiline_tag_column = leading_spaces(line);
                    multiline_tag_ignored = true;
                } else {
                    depth += opens - closes;
                    if depth < 0 { depth = 0; }
                }
                continue;
            }

            // Comments
            if trimmed.starts_with("<!--") { continue; }

            // Check for multiline opening tag
            if trimmed.starts_with('<') && !trimmed.starts_with("</") && !trimmed.starts_with("<!--") {
                if !trimmed.contains('>') {
                    in_multiline_tag = true;
                    multiline_tag_depth = depth;
                    multiline_tag_column = leading_spaces(line);
                    multiline_tag_ignored = false;
                    continue;
                }
            }

            // Compute expected indent
            let pre_depth = depth - closes;
            let check_depth = pre_depth.max(0) as usize;
            let actual = leading_spaces(line);
            let expected = check_depth * INDENT;

            if actual != expected {
                let msg = if actual == 1 {
                    format!("Expected indentation of {} spaces but found 1 whitespace.", expected)
                } else {
                    format!("Expected indentation of {} spaces but found {} spaces.", expected, actual)
                };
                ctx.diagnostic(msg, oxc::span::Span::new(line_start as u32, (line_start + actual.max(1)) as u32));
            }

            depth += opens - closes;
            if depth < 0 { depth = 0; }
        }
    }
}

fn count_opens(t: &str) -> i32 {
    let mut o = 0;
    if t.starts_with("{#") { o += 1; }
    if t.starts_with("{:") { o += 1; } // re-open after close
    if t.starts_with('<') && !t.starts_with("</") && !t.starts_with("<!--") {
        if !t.ends_with("/>") && !has_close_on_line(t) && t.contains('>') {
            o += 1;
        }
    }
    if t == "{" { o += 1; }
    o
}

fn count_closes(t: &str) -> i32 {
    let mut c = 0;
    if t.starts_with("</") || t.starts_with("{/") { c += 1; }
    if t.starts_with("{:") { c += 1; } // close before re-open
    if t == "}" { c += 1; }
    c
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ' || b == b'\t').count()
}

fn has_close_on_line(t: &str) -> bool {
    if t.ends_with("/>") { return true; }
    if let Some(gt) = t.find('>') {
        let name = t[1..gt].split_whitespace().next().unwrap_or("");
        if !name.is_empty() {
            return t.contains(&format!("</{}>", name));
        }
    }
    false
}
