//! `svelte/no-trailing-spaces` — disallow trailing whitespace at the end of lines.
//! 🔧 Fixable (Extension Rule)

use crate::linter::{LintContext, Rule, Fix};
use oxc::span::Span;
use std::collections::HashSet;

pub struct NoTrailingSpaces;

impl Rule for NoTrailingSpaces {
    fn name(&self) -> &'static str {
        "svelte/no-trailing-spaces"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Read options
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        let skip_blank_lines = opts
            .and_then(|o| o.get("skipBlankLines"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let ignore_comments = opts
            .and_then(|o| o.get("ignoreComments"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let source = ctx.source;

        // Build set of 1-indexed line numbers to ignore
        let mut ignored_lines: HashSet<usize> = HashSet::new();

        // Always exempt template literal interior lines.
        // For each template literal `...`, mark from the opening-backtick line
        // through (closing-backtick line - 1) inclusive.
        collect_template_literal_lines(source, &mut ignored_lines);

        // When ignoreComments: mark comment lines similarly.
        if ignore_comments {
            collect_comment_lines(source, &mut ignored_lines);
        }

        let mut offset = 0usize;
        for (line_idx, line) in source.lines().enumerate() {
            let line_num = line_idx + 1; // 1-indexed
            let line_start = offset;
            let line_end = offset + line.len();

            // Skip blank lines when option is set
            if skip_blank_lines && line.trim().is_empty() {
                offset = line_end + 1;
                continue;
            }

            if !ignored_lines.contains(&line_num) {
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
        }
    }
}

/// Collect 1-indexed line numbers that are inside template literals.
/// Marks from the opening-backtick line through (closing-backtick line - 1).
/// This exempts the opening line and all interior lines, but not the closing-backtick line.
fn collect_template_literal_lines(source: &str, ignored: &mut HashSet<usize>) {
    // Build a line-start offset table
    let line_starts = build_line_starts(source);

    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    // Track whether we're inside a <script> or <style> block to handle backticks
    // that appear in template HTML (not in JS). Actually, template literals only
    // occur in JS (inside <script> blocks). But we scan the whole source for
    // backticks — backticks in HTML attribute values would be unusual. We'll scan
    // all backticks conservatively.
    while i < len {
        // Skip string literals to avoid false positives from backticks inside '' or ""
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' { i += 1; } // skip escaped char
                i += 1;
            }
            i += 1;
            continue;
        }
        // Skip line comments to avoid backticks in // comments
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        // Skip block comments to avoid backticks in /* */ comments
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            let open_pos = i;
            i += 1;
            // Find the matching closing backtick, handling escapes and nested ${...}
            let mut depth = 0usize; // depth of ${ nesting
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2; // skip escaped char
                    continue;
                }
                if bytes[i] == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                    depth += 1;
                    i += 2;
                    continue;
                }
                if depth > 0 && bytes[i] == b'}' {
                    depth -= 1;
                    i += 1;
                    continue;
                }
                if depth == 0 && bytes[i] == b'`' {
                    break;
                }
                i += 1;
            }
            let close_pos = i;
            i += 1; // skip closing backtick

            // Determine line numbers
            let open_line = line_number_at(&line_starts, open_pos);   // 1-indexed
            let close_line = line_number_at(&line_starts, close_pos);  // 1-indexed

            // Mark lines from open_line through close_line - 1
            if close_line > open_line {
                for ln in open_line..close_line {
                    ignored.insert(ln);
                }
            }
            continue;
        }
        i += 1;
    }
}

/// Collect 1-indexed line numbers that are purely comment content.
/// - JS line comments `//`: mark the line itself
/// - JS block comments `/* */`: mark from start line through (end line - 1)
/// - HTML comments `<!-- -->`: mark from start line through (end line - 1)
fn collect_comment_lines(source: &str, ignored: &mut HashSet<usize>) {
    let line_starts = build_line_starts(source);
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // HTML comment <!-- ... -->
        if i + 3 < len && bytes[i] == b'<' && bytes[i+1] == b'!' && bytes[i+2] == b'-' && bytes[i+3] == b'-' {
            let start_pos = i;
            i += 4;
            while i + 2 < len && !(bytes[i] == b'-' && bytes[i+1] == b'-' && bytes[i+2] == b'>') {
                i += 1;
            }
            let end_pos = if i + 2 < len { i + 3 } else { len }; // position after -->
            i = end_pos;

            let start_line = line_number_at(&line_starts, start_pos);
            let end_line = line_number_at(&line_starts, end_pos.saturating_sub(1));
            // Mark from start_line through end_line - 1
            for ln in start_line..end_line {
                ignored.insert(ln);
            }
            continue;
        }
        // JS line comment //
        if i + 1 < len && bytes[i] == b'/' && bytes[i+1] == b'/' {
            let start_pos = i;
            while i < len && bytes[i] != b'\n' { i += 1; }
            let line_num = line_number_at(&line_starts, start_pos);
            ignored.insert(line_num);
            continue;
        }
        // JS block comment /* */
        if i + 1 < len && bytes[i] == b'/' && bytes[i+1] == b'*' {
            let start_pos = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i+1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
            let end_pos = i;

            let start_line = line_number_at(&line_starts, start_pos);
            let end_line = line_number_at(&line_starts, end_pos.saturating_sub(1));
            // Mark from start_line through end_line - 1
            for ln in start_line..end_line {
                ignored.insert(ln);
            }
            continue;
        }
        i += 1;
    }
}

/// Build a sorted list of byte offsets where each line starts (0-indexed by line).
fn build_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, ch) in source.char_indices() {
        if ch == '\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Return the 1-indexed line number for a given byte offset using the line_starts table.
fn line_number_at(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(idx) => idx + 1,
        Err(idx) => idx, // idx is the insertion point; line is idx (1-indexed = idx)
    }
}
