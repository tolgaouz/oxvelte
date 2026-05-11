//! `svelte/max-attributes-per-line` — enforce the maximum number of attributes per line.
//! 🔧 Fixable

use crate::ast::{Attribute, DirectiveKind, TemplateNode};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};

pub struct MaxAttributesPerLine;

impl Rule for MaxAttributesPerLine {
    fn name(&self) -> &'static str {
        "svelte/max-attributes-per-line"
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
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .cloned();

        let get_opt = |key: &str| {
            opts.as_ref()
                .and_then(|o| o.get(key))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(1)
        };
        let singleline_max = get_opt("singleline");
        let multiline_max = get_opt("multiline");

        let source = ctx.source;
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter(|(_, b)| *b == b'\n')
                    .map(|(i, _)| i + 1),
            )
            .collect();

        let offset_to_line = |offset: usize| -> usize {
            line_starts
                .partition_point(|&start| start <= offset)
                .saturating_sub(1)
        };

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let attrs = &el.attributes;
                if attrs.is_empty() {
                    return;
                }

                let el_start = el.span.start as usize;

                let opening_tag_end_line = offset_to_line(el.start_tag_end as usize);

                let opening_start_line = offset_to_line(el_start);
                let is_singleline = opening_start_line == opening_tag_end_line;

                if is_singleline {
                    if attrs.len() > singleline_max {
                        report_attribute(singleline_max, attrs, source, ctx);
                    }
                } else {
                    let groups = group_attr_indices_by_line(attrs, &offset_to_line);
                    for group in groups {
                        if group.len() > multiline_max {
                            report_attribute(group[multiline_max], attrs, source, ctx);
                        }
                    }
                }
            }
        });
    }
}

fn report_attribute(index: usize, attrs: &[Attribute], source: &str, ctx: &mut LintContext<'_>) {
    let Some(attr) = attrs.get(index) else {
        return;
    };
    let name = attr_name(attr, source);
    let span = attr_span(attr);
    let fix_start = if index > 0 {
        attr_span(&attrs[index - 1]).end
    } else {
        span.start
    };
    ctx.diagnostic_with_fix(
        format!("'{}' should be on a new line.", name),
        span,
        Fix {
            span: oxc::span::Span::new(fix_start, span.start),
            replacement: "\n".to_string(),
        },
    );
}

fn attr_name(attr: &Attribute, source: &str) -> String {
    match attr {
        Attribute::NormalAttribute { name, .. } => name.clone(),
        Attribute::Spread { span } => {
            let start = span.start as usize;
            let end = span.end as usize;
            if end <= source.len() {
                source[start..end].to_string()
            } else {
                "{...}".to_string()
            }
        }
        Attribute::Directive {
            kind,
            name,
            modifiers,
            ..
        } => {
            let mut label = format!("{}:{name}", directive_prefix(kind));
            for modifier in modifiers {
                label.push('|');
                label.push_str(modifier);
            }
            label
        }
    }
}

fn directive_prefix(kind: &DirectiveKind) -> &'static str {
    match kind {
        DirectiveKind::EventHandler => "on",
        DirectiveKind::Binding => "bind",
        DirectiveKind::Class => "class",
        DirectiveKind::StyleDirective => "style",
        DirectiveKind::Use => "use",
        DirectiveKind::Transition => "transition",
        DirectiveKind::In => "in",
        DirectiveKind::Out => "out",
        DirectiveKind::Animate => "animate",
        DirectiveKind::Let => "let",
    }
}

fn attr_span(attr: &Attribute) -> oxc::span::Span {
    match attr {
        Attribute::NormalAttribute { span, .. }
        | Attribute::Spread { span }
        | Attribute::Directive { span, .. } => *span,
    }
}

fn group_attr_indices_by_line<F>(attrs: &[Attribute], offset_to_line: &F) -> Vec<Vec<usize>>
where
    F: Fn(usize) -> usize,
{
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, attr) in attrs.iter().enumerate() {
        let start_line = offset_to_line(attr_span(attr).start as usize);
        let same = groups
            .last()
            .and_then(|g| g.first())
            .map_or(false, |&first| {
                offset_to_line(attr_span(&attrs[first]).end as usize) == start_line
            });
        if same {
            groups.last_mut().unwrap().push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    groups
}
