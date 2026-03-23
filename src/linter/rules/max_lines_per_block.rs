//! `svelte/max-lines-per-block` — enforce a maximum number of lines in script/style blocks.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

/// Default maximum number of lines allowed per block.
const DEFAULT_MAX_LINES: usize = 200;

pub struct MaxLinesPerBlock;

/// Count lines in content after trimming leading/trailing whitespace.
/// Returns 0 for empty/whitespace-only content.
fn count_lines(content: &str, skip_blank_lines: bool, skip_comments: bool) -> usize {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    if !skip_blank_lines && !skip_comments {
        return lines.len();
    }
    lines.iter().filter(|line| {
        let l = line.trim();
        if skip_blank_lines && l.is_empty() {
            return false;
        }
        if skip_comments && is_comment_only_line(l) {
            return false;
        }
        true
    }).count()
}

/// Check if a line is a comment-only line (JS/CSS comments).
fn is_comment_only_line(line: &str) -> bool {
    // Single-line comment
    if line.starts_with("//") {
        return true;
    }
    // Single-line block comment: /* ... */
    if line.starts_with("/*") && line.ends_with("*/") {
        return true;
    }
    false
}

/// Check if a line is an HTML comment-only line.
fn is_html_comment_only_line(line: &str) -> bool {
    if line.starts_with("<!--") && line.ends_with("-->") {
        return true;
    }
    is_comment_only_line(line)
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

        // Determine if we're using per-block config or defaults
        let has_per_block = script_limit.is_some() || style_limit.is_some() || template_limit.is_some();

        // Resolve effective limits: if per-block config exists, only check configured blocks
        let eff_script = if has_per_block { script_limit } else { Some(DEFAULT_MAX_LINES) };
        let eff_style = if has_per_block { style_limit } else { Some(DEFAULT_MAX_LINES) };
        let eff_template = if has_per_block { template_limit } else { None };

        // Check instance script block
        if let Some(max) = eff_script {
            if let Some(script) = &ctx.ast.instance {
                let line_count = count_lines(&script.content, skip_blank_lines, skip_comments);
                if line_count > max {
                    ctx.diagnostic(
                        format!(
                            "<script> block has too many lines ({line_count}). Maximum allowed is {max}."
                        ),
                        script.span,
                    );
                }
            }
        }

        // Check module script block
        if let Some(max) = eff_script {
            if let Some(module) = &ctx.ast.module {
                let line_count = count_lines(&module.content, skip_blank_lines, skip_comments);
                if line_count > max {
                    ctx.diagnostic(
                        format!(
                            "<script> block has too many lines ({line_count}). Maximum allowed is {max}."
                        ),
                        module.span,
                    );
                }
            }
        }

        // Check style block
        if let Some(max) = eff_style {
            if let Some(style) = &ctx.ast.css {
                let line_count = count_lines(&style.content, skip_blank_lines, skip_comments);
                if line_count > max {
                    ctx.diagnostic(
                        format!(
                            "<style> block has too many lines ({line_count}). Maximum allowed is {max}."
                        ),
                        style.span,
                    );
                }
            }
        }

        // Check template block
        if let Some(max) = eff_template {
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
    // When skipping, we need to filter individual lines
    // Template line count = number of \n transitions between kept lines
    let lines: Vec<&str> = content.split('\n').collect();
    let kept_count = lines.iter().filter(|line| {
        let l = line.trim();
        if skip_blank_lines && l.is_empty() {
            return false;
        }
        if skip_comments && is_html_comment_only_line(l) {
            return false;
        }
        true
    }).count();
    // Template lines are counted as newline count, which is kept_count - 1 for the
    // transitions between lines, but we need at least the count if there are lines
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
