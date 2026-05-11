//! `svelte/html-closing-bracket-new-line` — require or disallow a newline before
//! the closing bracket of elements.
//! 🔧 Fixable

use crate::ast::Element;
use crate::ast::TemplateNode;
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use oxc::span::Span;

pub struct HtmlClosingBracketNewLine;

impl Rule for HtmlClosingBracketNewLine {
    fn name(&self) -> &'static str {
        "svelte/html-closing-bracket-new-line"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let get_str = |key: &str| opts.and_then(|o| o.get(key)).and_then(|v| v.as_str());
        let singleline_expect_newline = get_str("singleline")
            .map(|s| s == "always")
            .unwrap_or(false);
        let multiline_expect_newline = get_str("multiline").map(|s| s == "always").unwrap_or(true);
        let sc = opts.and_then(|o| o.get("selfClosingTag"));
        let sc_get = |key: &str| {
            sc.and_then(|o| o.get(key))
                .and_then(|v| v.as_str())
                .map(|s| s == "always")
        };
        let (sc_singleline, sc_multiline) = (sc_get("singleline"), sc_get("multiline"));
        let line_starts = build_line_starts(ctx.source);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let el = match node {
                TemplateNode::Element(el) => el,
                _ => return,
            };

            let Some(start_data) = start_tag_data(el, &line_starts) else {
                return;
            };
            let is_self_closing = el.self_closing;
            let expect_newline = if start_data.multiline {
                if is_self_closing {
                    sc_multiline.unwrap_or(multiline_expect_newline)
                } else {
                    multiline_expect_newline
                }
            } else if is_self_closing {
                sc_singleline.unwrap_or(singleline_expect_newline)
            } else {
                singleline_expect_newline
            };
            report_if_needed(ctx, start_data, if expect_newline { 1 } else { 0 }, true);

            if let Some(end_data) = end_tag_data(el, &line_starts) {
                let expect_newline = if end_data.multiline {
                    multiline_expect_newline
                } else {
                    singleline_expect_newline
                };
                report_if_needed(ctx, end_data, if expect_newline { 1 } else { 0 }, false);
            }
        });
    }
}

#[derive(Clone, Copy)]
struct BracketData {
    actual: usize,
    multiline: bool,
    replace_span: Span,
    report_span: Span,
}

fn start_tag_data(el: &Element<'_>, line_starts: &[usize]) -> Option<BracketData> {
    let bracket = el.start_tag_end;
    let end_token_start = if el.self_closing {
        bracket.checked_sub(1)?
    } else {
        bracket
    };
    let prev_end = el
        .attributes
        .last()
        .map(attr_span)
        .map(|span| span.end)
        .unwrap_or(el.name_span.end);
    bracket_data(line_starts, el.span.start, prev_end, end_token_start)
}

fn end_tag_data(el: &Element<'_>, line_starts: &[usize]) -> Option<BracketData> {
    let end_tag = el.end_tag_span?;
    let name_end = end_tag.start + 2 + el.name.len() as u32;
    let bracket = end_tag.end.checked_sub(1)?;
    if name_end > bracket {
        return None;
    }
    bracket_data(line_starts, end_tag.start, name_end, bracket)
}

fn bracket_data(
    line_starts: &[usize],
    node_start: u32,
    prev_end: u32,
    end_token_start: u32,
) -> Option<BracketData> {
    if prev_end > end_token_start {
        return None;
    }
    let prev_line = line_number_at(line_starts, prev_end as usize);
    let end_token_line = line_number_at(line_starts, end_token_start as usize);
    let actual = end_token_line.saturating_sub(prev_line);
    let multiline = line_number_at(line_starts, node_start as usize) != prev_line;
    let replace_span = Span::new(prev_end, end_token_start);
    Some(BracketData {
        actual,
        multiline,
        replace_span,
        report_span: replace_span,
    })
}

fn attr_span(attr: &crate::ast::Attribute) -> Span {
    match attr {
        crate::ast::Attribute::NormalAttribute { span, .. }
        | crate::ast::Attribute::Spread { span }
        | crate::ast::Attribute::Directive { span, .. } => *span,
    }
}

fn build_line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(idx, _)| idx + 1),
        )
        .collect()
}

fn line_number_at(line_starts: &[usize], offset: usize) -> usize {
    line_starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1)
}

fn report_if_needed(
    ctx: &mut LintContext<'_>,
    data: BracketData,
    expected: usize,
    can_fix_add_linebreak: bool,
) {
    if data.actual == expected {
        return;
    }

    let message = format!(
        "Expected {} before closing bracket, but {} found.",
        phrase(expected),
        phrase(data.actual)
    );
    if expected > 0 && !can_fix_add_linebreak {
        ctx.diagnostic(message, data.report_span);
        return;
    }

    ctx.diagnostic_with_fix(
        message,
        data.report_span,
        Fix {
            span: data.replace_span,
            replacement: "\n".repeat(expected),
        },
    );
}

fn phrase(line_breaks: usize) -> String {
    match line_breaks {
        0 => "no line breaks".to_string(),
        1 => "1 line break".to_string(),
        n => format!("{n} line breaks"),
    }
}
