//! `svelte/max-attributes-per-line` — enforce the maximum number of attributes per line.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, DirectiveKind, AttributeValue, TemplateNode};

pub struct MaxAttributesPerLine;

impl Rule for MaxAttributesPerLine {
    fn name(&self) -> &'static str {
        "svelte/max-attributes-per-line"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Read singleline and multiline options from config.
        // Config is passed as options: [{ "singleline": N, "multiline": N }]
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .cloned();

        let get_opt = |key: &str| opts.as_ref()
            .and_then(|o| o.get(key)).and_then(|v| v.as_u64())
            .map(|v| v as usize).unwrap_or(1);
        let singleline_max = get_opt("singleline");
        let multiline_max = get_opt("multiline");

        // Build line-start table for span → line number conversion.
        let source = ctx.source;
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(
                source.bytes()
                    .enumerate()
                    .filter(|(_, b)| *b == b'\n')
                    .map(|(i, _)| i + 1),
            )
            .collect();

        let offset_to_line = |offset: usize| -> usize {
            line_starts.partition_point(|&start| start <= offset).saturating_sub(1)
        };

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let attrs = &el.attributes;
                if attrs.is_empty() {
                    return;
                }

                // Determine whether the opening tag is singleline or multiline.
                // The opening tag spans from el.span.start up to (and including) the first `>`.
                let el_start = el.span.start as usize;
                let el_end = el.span.end as usize;

                let tag_close = source.get(el_start..el_end)
                    .and_then(|s| s.find('>')).map(|p| el_start + p).unwrap_or(el_end);
                let opening_tag_end_line = offset_to_line(tag_close);

                let opening_start_line = offset_to_line(el_start);
                let is_singleline = opening_start_line == opening_tag_end_line;

                if is_singleline {
                    if let Some(attr) = attrs.get(singleline_max) {
                        let name = attr_name(attr, source);
                        ctx.diagnostic(format!("'{}' should be on a new line.", name), attr_span(attr));
                    }
                } else {
                    for group in group_attrs_by_line(attrs, &offset_to_line) {
                        if let Some(attr) = group.get(multiline_max) {
                            let name = attr_name(attr, source);
                            ctx.diagnostic(format!("'{}' should be on a new line.", name), attr_span(attr));
                        }
                    }
                }
            }
        });
    }
}

/// Get the display name of an attribute (as it appears in source).
fn attr_name(attr: &Attribute, source: &str) -> String {
    match attr {
        Attribute::NormalAttribute { name, value, .. } => {
            // Shorthand: {expr} where name == expr string
            match value {
                AttributeValue::Expression(expr) if expr == name => {
                    format!("{{{}}}", name)
                }
                _ => name.clone(),
            }
        }
        Attribute::Spread { span } => {
            // Extract the spread text from source, e.g. `{...attrs}`
            let start = span.start as usize;
            let end = span.end as usize;
            if end <= source.len() {
                source[start..end].to_string()
            } else {
                "{...}".to_string()
            }
        }
        Attribute::Directive { kind, name, modifiers, .. } => {
            let prefix = directive_prefix(kind);
            if modifiers.is_empty() {
                format!("{}:{}", prefix, name)
            } else {
                format!("{}:{}|{}", prefix, name, modifiers.join("|"))
            }
        }
    }
}

/// Get the span of an attribute.
fn attr_span(attr: &Attribute) -> oxc::span::Span {
    match attr {
        Attribute::NormalAttribute { span, .. } => *span,
        Attribute::Spread { span } => *span,
        Attribute::Directive { span, .. } => *span,
    }
}

/// Map DirectiveKind to its source prefix string.
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

fn group_attrs_by_line<'a, F>(attrs: &'a [Attribute], offset_to_line: &F) -> Vec<Vec<&'a Attribute>>
where F: Fn(usize) -> usize {
    let mut groups: Vec<Vec<&Attribute>> = Vec::new();
    for attr in attrs {
        let start_line = offset_to_line(attr_span(attr).start as usize);
        let same = groups.last().and_then(|g| g.first())
            .map_or(false, |first| offset_to_line(attr_span(first).end as usize) == start_line);
        if same { groups.last_mut().unwrap().push(attr); }
        else { groups.push(vec![attr]); }
    }
    groups
}
