//! `svelte/max-lines-per-block` — enforce a maximum number of lines in script/style blocks.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct MaxLinesPerBlock;

/// Count lines in content after trimming leading/trailing whitespace.
/// Supports skipping blank lines and comment-only lines (including multi-line block comments).
/// Returns 0 for empty/whitespace-only content.
fn count_lines(content: &str, skip_blank_lines: bool, skip_comments: bool) -> usize {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return 0;
    }
    if !skip_blank_lines && !skip_comments {
        return trimmed.lines().count();
    }

    let lines: Vec<&str> = trimmed.lines().collect();

    if !skip_comments {
        // Only skip blank lines — no comment state needed.
        return lines.iter().filter(|line| !line.trim().is_empty()).count();
    }

    // Track multi-line block comment state across lines.
    let mut in_block_comment = false;
    let mut count = 0usize;

    for line in &lines {
        let l = line.trim();

        if skip_blank_lines && l.is_empty() {
            continue;
        }

        // Determine whether this line is entirely within a comment context.
        let is_comment = classify_line(l, &mut in_block_comment);

        if !is_comment {
            count += 1;
        }
    }

    count
}

/// Classify a trimmed line as a full-comment line or not, updating `in_block_comment` state.
///
/// Returns `true` when the line contributes only comment text (and no code).
fn classify_line(line: &str, in_block_comment: &mut bool) -> bool {
    if *in_block_comment {
        // We're inside a `/* … */` block comment that started on a previous line.
        if let Some(end) = line.find("*/") {
            *in_block_comment = false;
            // After `*/`, check whether any non-whitespace code follows.
            let after = line[end + 2..].trim();
            // The line opened inside a block comment and closed it — check for trailing code.
            return after.is_empty();
        }
        // Entire line is inside the block comment.
        return true;
    }

    // Not in a block comment — classify from the start of the line.

    // Single-line `//` comment.
    if line.starts_with("//") {
        return true;
    }

    // HTML comment: `<!-- … -->` on a single line.
    if line.starts_with("<!--") {
        if line.contains("-->") {
            // Single-line HTML comment — only comment if nothing comes after `-->`.
            let after_idx = line.find("-->").unwrap() + 3;
            return line[after_idx..].trim().is_empty();
        }
        // Multi-line HTML comment start — treat whole line as comment (no code precedes it).
        // We don't track HTML comment state across lines (rare in Svelte templates).
        return true;
    }

    // Block comment start `/* … */`.
    if line.starts_with("/*") {
        if let Some(end) = line.find("*/") {
            // Closed on the same line — check for trailing code.
            let after = line[end + 2..].trim();
            if after.is_empty() {
                return true;
            }
            // Code follows after `*/` on the same line — not a pure comment line.
            return false;
        }
        // Block comment opens and does not close on this line.
        *in_block_comment = true;
        return true;
    }

    // Lines that are continuation lines of a block comment, e.g. ` * foo`.
    // These only appear inside a block comment started on a previous line, which is handled
    // above.  At this point `in_block_comment` is false, so this is regular code.

    false
}

impl Rule for MaxLinesPerBlock {
    fn name(&self) -> &'static str {
        "svelte/max-lines-per-block"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .cloned();

        let skip_comments = opts.as_ref()
            .and_then(|o| o.get("skipComments"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let skip_blank_lines = opts.as_ref()
            .and_then(|o| o.get("skipBlankLines"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // When a block type has no configured limit, it is not checked (no default).
        let script_limit = opts.as_ref()
            .and_then(|o| o.get("script"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let style_limit = opts.as_ref()
            .and_then(|o| o.get("style"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let template_limit = opts.as_ref()
            .and_then(|o| o.get("template"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let blocks: Vec<(&str, oxc::span::Span, Option<usize>, &str)> = [
            ctx.ast.instance.as_ref().map(|s| (s.content.as_str(), s.span, script_limit, "script")),
            ctx.ast.module.as_ref().map(|s| (s.content.as_str(), s.span, script_limit, "script")),
            ctx.ast.css.as_ref().map(|s| (s.content.as_str(), s.span, style_limit, "style")),
        ].into_iter().flatten().collect();
        for (content, span, limit, tag) in blocks {
            if let Some(max) = limit {
                let line_count = count_lines(content, skip_blank_lines, skip_comments);
                if line_count > max {
                    ctx.diagnostic(
                        format!("<{tag}> block has too many lines ({line_count}). Maximum allowed is {max}."),
                        span,
                    );
                }
            }
        }

        // Check template block
        if let Some(max) = template_limit {
            let source = ctx.source;
            let template_content = extract_template_content(source, ctx);
            let line_count = count_template_lines(&template_content, skip_blank_lines, skip_comments);
            if line_count > max {
                // Span at the end of the last script tag (template start)
                let template_start = find_template_start(source, ctx);
                ctx.diagnostic(
                    format!(
                        "template block has too many lines ({line_count}). Maximum allowed is {max}."
                    ),
                    Span::new(template_start, template_start + 1),
                );
            }
        }
    }
}

/// Extract the template content (parts of source not inside <script> or <style> blocks).
fn extract_template_content(source: &str, ctx: &LintContext) -> String {
    let mut regions_to_exclude: Vec<(usize, usize)> = Vec::new();

    if let Some(script) = &ctx.ast.instance {
        regions_to_exclude.push((script.span.start as usize, script.span.end as usize));
    }
    if let Some(module) = &ctx.ast.module {
        regions_to_exclude.push((module.span.start as usize, module.span.end as usize));
    }
    if let Some(style) = &ctx.ast.css {
        regions_to_exclude.push((style.span.start as usize, style.span.end as usize));
    }

    regions_to_exclude.sort_by_key(|&(start, _)| start);

    let mut result = String::new();
    let mut pos = 0;
    for (start, end) in &regions_to_exclude {
        if pos < *start {
            result.push_str(&source[pos..*start]);
        }
        pos = *end;
    }
    if pos < source.len() {
        result.push_str(&source[pos..]);
    }
    result
}

/// Count template lines: number of newlines in the template region.
fn count_template_lines(content: &str, skip_blank_lines: bool, skip_comments: bool) -> usize {
    if !skip_blank_lines && !skip_comments {
        return content.matches('\n').count();
    }
    // When skipping, we need to filter individual lines.
    // Template line count = number of \n transitions between kept lines.
    let lines: Vec<&str> = content.split('\n').collect();
    let kept_count = lines.iter().filter(|line| {
        let l = line.trim();
        if skip_blank_lines && l.is_empty() {
            return false;
        }
        if skip_comments {
            // Use a fresh state for each template line — HTML comments don't nest with
            // JS block comments in templates; simple single-line detection is sufficient here.
            let mut dummy = false;
            if classify_line(l, &mut dummy) && !dummy {
                return false;
            }
        }
        true
    }).count();
    // Template lines are counted as newline count, which is kept_count - 1 for the
    // transitions between lines, but we need at least the count if there are lines.
    if kept_count <= 1 {
        kept_count
    } else {
        kept_count - 1
    }
}

/// Find the start position for the template span (after the last script close tag).
fn find_template_start(source: &str, ctx: &LintContext) -> u32 {
    // Find the end of the last script block
    let mut last_end = 0u32;
    if let Some(script) = &ctx.ast.instance {
        if script.span.end > last_end {
            last_end = script.span.end;
        }
    }
    if let Some(module) = &ctx.ast.module {
        if module.span.end > last_end {
            last_end = module.span.end;
        }
    }
    // Point to the end of the closing tag (the > character)
    if last_end > 0 {
        last_end - 1
    } else {
        0
    }
}
