//! `svelte/first-attribute-linebreak` — enforce the location of first attribute.
//! 🔧 Fixable

use crate::ast::{Attribute, TemplateNode};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};

pub struct FirstAttributeLinebreak;

fn attr_span(attr: &Attribute) -> oxc::span::Span {
    match attr {
        Attribute::NormalAttribute { span, .. }
        | Attribute::Spread { span }
        | Attribute::Directive { span, .. } => *span,
    }
}

impl Rule for FirstAttributeLinebreak {
    fn name(&self) -> &'static str {
        "svelte/first-attribute-linebreak"
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
        let get_mode = |key, default| {
            opts.and_then(|v| v.get(key))
                .and_then(|v| v.as_str())
                .unwrap_or(default)
                .to_string()
        };
        let multiline_mode = get_mode("multiline", "below");
        let singleline_mode = get_mode("singleline", "beside");
        let src = ctx.source;
        let line_starts = build_line_starts(src);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            if el.attributes.is_empty() {
                return;
            }

            let first_start = attr_span(el.attributes.first().unwrap()).start as usize;
            let last_end = attr_span(el.attributes.last().unwrap()).end as usize;
            let is_single =
                line_number_at(&line_starts, first_start) == line_number_at(&line_starts, last_end);
            let mode = if is_single {
                &singleline_mode
            } else {
                &multiline_mode
            };

            let first_attr = el.attributes.first().unwrap();
            let first_span = attr_span(first_attr);
            let name_end = el.name_span.end as usize;
            let on_new_line =
                line_number_at(&line_starts, name_end) != line_number_at(&line_starts, first_start);

            if mode == "below" && !on_new_line {
                ctx.diagnostic_with_fix(
                    "Expected a linebreak before this attribute.",
                    first_span,
                    Fix {
                        span: oxc::span::Span::new(el.name_span.end, first_span.start),
                        replacement: "\n".to_string(),
                    },
                );
            } else if mode == "beside" && on_new_line {
                ctx.diagnostic_with_fix(
                    "Expected no linebreak before this attribute.",
                    first_span,
                    Fix {
                        span: oxc::span::Span::new(el.name_span.end, first_span.start),
                        replacement: " ".to_string(),
                    },
                );
            }
        });
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
