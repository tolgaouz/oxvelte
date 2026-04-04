//! `svelte/indent` — enforce consistent indentation (2 spaces default).
//! 🔧 Fixable
//!
//! Checks template content indentation. Skips script/style blocks,
//! multiline tag attributes, and prettier-ignore sections.

use crate::linter::{LintContext, Rule};

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
        let mut multiline_tag_ignored = false;
        let mut multiline_tag_column = 0usize;
        let mut multiline_brace_depth = 0i32;
        let mut in_script = false;
        let mut script_base_depth = 0i32;

        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let indent_size: usize = opts.and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(2);
        let use_tabs = opts.and_then(|v| v.as_str()).map(|s| s == "tab").unwrap_or(false);
        let indent = if use_tabs { 1 } else { indent_size };

        let indent_script = opts
            .and_then(|o| o.get("indentScript"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        for &line in &lines {
            let line_start = offset;
            offset += line.len() + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }

            if let Some(end) = skip_tag {
                if trimmed.starts_with(end) { skip_tag = None; }
                continue;
            }
            if trimmed.starts_with("<style") { skip_tag = Some("</style"); continue; }
            if trimmed.starts_with("<template") && trimmed.contains("lang=") {
                skip_tag = Some("</template");
                continue;
            }
            if trimmed.starts_with("<script") || trimmed.starts_with("</script") {
                if skip_next_line { skip_next_line = false; }
                if trimmed.starts_with("<script") && !trimmed.ends_with("/>") {
                    in_script = true;
                    if indent_script {
                        depth += 1;
                    }
                    script_base_depth = depth;
                }
                if trimmed.starts_with("</script") {
                    in_script = false;
                    if indent_script {
                        depth -= 1;
                        if depth < 0 { depth = 0; }
                    }
                }
                continue;
            }

            if trimmed.contains("prettier-ignore") {
                skip_next_line = true;
                continue;
            }

            if in_multiline_tag {
                let is_end = trimmed.ends_with(">") || trimmed.ends_with("/>") || trimmed == ">" || trimmed == "/>";
                if !is_end && !multiline_tag_ignored && multiline_brace_depth == 0 {
                    let actual = leading_spaces(line);
                    let expected = multiline_tag_column + indent;
                    let first_char = trimmed.chars().next().unwrap_or(' ');
                    let is_simple_attr = first_char.is_ascii_alphabetic() || first_char == '_' || first_char == '$';
                    if is_simple_attr && actual != expected && actual < expected + indent {
                        let msg = format!("Expected indentation of {} spaces but found {} spaces.", expected, actual);
                        ctx.diagnostic(msg, oxc::span::Span::new(line_start as u32, (line_start + actual.max(1)) as u32));
                    }
                }
                if !is_end {
                    for c in trimmed.chars() {
                        if c == '{' { multiline_brace_depth += 1; }
                        if c == '}' { multiline_brace_depth -= 1; if multiline_brace_depth < 0 { multiline_brace_depth = 0; } }
                    }
                }
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

            if skip_next_line {
                skip_next_line = false;
                if trimmed.starts_with('<') && !trimmed.starts_with("</") && !trimmed.starts_with("<!--") && !trimmed.contains('>') {
                    in_multiline_tag = true;
                    multiline_tag_column = leading_spaces(line);
                    multiline_tag_ignored = false; // still check attributes of ignored tags
                    multiline_brace_depth = 0;
                } else {
                    depth += opens - closes;
                    if depth < 0 { depth = 0; }
                }
                continue;
            }

            if in_script {
                let actual = leading_spaces(line);
                let base = (script_base_depth.max(0) as usize) * indent;
                if !indent_script {
                    if actual != 0 && trimmed.starts_with("const ") || trimmed.starts_with("let ") || trimmed.starts_with("var ")
                        || trimmed.starts_with("function ") || trimmed.starts_with("import ") || trimmed.starts_with("export ")
                        || trimmed.starts_with("type ") || trimmed.starts_with("interface ") || trimmed.starts_with("class ")
                        || trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("$")
                    {
                        if actual != 0 {
                            let msg = format!("Expected indentation of 0 spaces but found {} spaces.", actual);
                            ctx.diagnostic(msg, oxc::span::Span::new(line_start as u32, (line_start + actual.max(1)) as u32));
                        }
                    }
                } else if actual < base {
                    let msg = format!("Expected indentation of {} spaces but found {} spaces.", base, actual);
                    ctx.diagnostic(msg, oxc::span::Span::new(line_start as u32, (line_start + actual.max(1)) as u32));
                }
                continue;
            }

            if trimmed.starts_with("<!--") { continue; }

            if trimmed.starts_with('<') && !trimmed.starts_with("</") && !trimmed.starts_with("<!--") {
                if !trimmed.contains('>') {
                    in_multiline_tag = true;
                    multiline_tag_column = leading_spaces(line);
                    multiline_tag_ignored = false;
                    multiline_brace_depth = 0;
                    continue;
                }
            }

            let pre_depth = depth - closes;
            let check_depth = pre_depth.max(0) as usize;
            let actual = leading_spaces(line);
            let expected = check_depth * indent;

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
