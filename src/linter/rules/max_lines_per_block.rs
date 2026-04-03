//! `svelte/max-lines-per-block` — enforce a maximum number of lines in script/style blocks.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct MaxLinesPerBlock;

fn count_lines(content: &str, skip_blank: bool, skip_comments: bool) -> usize {
    let trimmed = content.trim();
    if trimmed.is_empty() { return 0; }
    if !skip_blank && !skip_comments { return trimmed.lines().count(); }
    if !skip_comments { return trimmed.lines().filter(|l| !l.trim().is_empty()).count(); }
    let mut in_block = false;
    trimmed.lines().filter(|line| {
        let l = line.trim();
        !(skip_blank && l.is_empty()) && !classify_line(l, &mut in_block)
    }).count()
}

fn classify_line(line: &str, in_block: &mut bool) -> bool {
    if *in_block {
        return if let Some(end) = line.find("*/") { *in_block = false; line[end + 2..].trim().is_empty() } else { true };
    }
    if line.starts_with("//") { return true; }
    if line.starts_with("<!--") {
        return !line.contains("-->") || line[line.find("-->").unwrap() + 3..].trim().is_empty();
    }
    if line.starts_with("/*") {
        return if let Some(end) = line.find("*/") { line[end + 2..].trim().is_empty() } else { *in_block = true; true };
    }
    false
}

impl Rule for MaxLinesPerBlock {
    fn name(&self) -> &'static str {
        "svelte/max-lines-per-block"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref().and_then(|v| v.as_array()).and_then(|a| a.first());
        let get_bool = |k| opts.and_then(|o| o.get(k)).and_then(|v| v.as_bool()).unwrap_or(false);
        let get_limit = |k| opts.and_then(|o| o.get(k)).and_then(|v| v.as_u64()).map(|v| v as usize);
        let (skip_blank, skip_comments) = (get_bool("skipBlankLines"), get_bool("skipComments"));
        let (script_limit, style_limit, template_limit) = (get_limit("script"), get_limit("style"), get_limit("template"));

        let blocks: Vec<(&str, oxc::span::Span, Option<usize>, &str)> = [
            ctx.ast.instance.as_ref().map(|s| (s.content.as_str(), s.span, script_limit, "script")),
            ctx.ast.module.as_ref().map(|s| (s.content.as_str(), s.span, script_limit, "script")),
            ctx.ast.css.as_ref().map(|s| (s.content.as_str(), s.span, style_limit, "style")),
        ].into_iter().flatten().collect();
        for (content, span, limit, tag) in blocks {
            if let Some(max) = limit {
                let lc = count_lines(content, skip_blank, skip_comments);
                if lc > max { ctx.diagnostic(format!("<{tag}> block has too many lines ({lc}). Maximum allowed is {max}."), span); }
            }
        }
        if let Some(max) = template_limit {
            let tc = extract_template_content(ctx.source, ctx);
            let lc = count_template_lines(&tc, skip_blank, skip_comments);
            if lc > max {
                let ts = [&ctx.ast.instance, &ctx.ast.module].iter().filter_map(|s| s.as_ref()).map(|s| s.span.end).max().map(|e| e - 1).unwrap_or(0);
                ctx.diagnostic(format!("template block has too many lines ({lc}). Maximum allowed is {max}."), Span::new(ts, ts + 1));
            }
        }
    }
}

fn extract_template_content(source: &str, ctx: &LintContext) -> String {
    let mut regions = Vec::new();
    for s in [&ctx.ast.instance, &ctx.ast.module].iter().filter_map(|s| s.as_ref()) {
        regions.push((s.span.start as usize, s.span.end as usize));
    }
    if let Some(s) = &ctx.ast.css { regions.push((s.span.start as usize, s.span.end as usize)); }
    regions.sort_by_key(|&(s, _)| s);
    let mut result = String::new();
    let mut pos = 0;
    for (start, end) in &regions {
        if pos < *start { result.push_str(&source[pos..*start]); }
        pos = *end;
    }
    if pos < source.len() { result.push_str(&source[pos..]); }
    result
}

fn count_template_lines(content: &str, skip_blank: bool, skip_comments: bool) -> usize {
    if !skip_blank && !skip_comments { return content.matches('\n').count(); }
    let kept = content.split('\n').filter(|line| {
        let l = line.trim();
        !(skip_blank && l.is_empty()) && !(skip_comments && { let mut d = false; classify_line(l, &mut d) && !d })
    }).count();
    if kept <= 1 { kept } else { kept - 1 }
}
