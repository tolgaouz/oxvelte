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

        let singleline_max = opts.as_ref()
            .and_then(|o| o.get("singleline"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(1);

        let multiline_max = opts.as_ref()
            .and_then(|o| o.get("multiline"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(1);

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

                // Find the end of the opening tag (first `>` that closes the start tag).
                let opening_tag_end_line = if el_end <= source.len() {
                    let tag_src = &source[el_start..el_end];
                    if let Some(close_pos) = tag_src.find('>') {
                        let opening_end_offset = el_start + close_pos;
                        offset_to_line(opening_end_offset)
                    } else {
                        offset_to_line(el_end)
                    }
                } else {
                    offset_to_line(el_end)
                };

                let opening_start_line = offset_to_line(el_start);
                let is_singleline = opening_start_line == opening_tag_end_line;

                if is_singleline {
                    // For singleline elements: report the attribute at index `singleline_max`
                    // (the first one that exceeds the limit).
                    if attrs.len() > singleline_max {
                        if let Some(attr) = attrs.get(singleline_max) {
                            let name = attr_name(attr, source);
                            ctx.diagnostic(
                                format!("'{}' should be on a new line.", name),
                                attr_span(attr),
                            );
                        }
                    }
                } else {
                    // For multiline elements: group attributes by their line number,
                    // then for each line group that exceeds the limit, report the attr
                    // at index `multiline_max` within that group.
                    let groups = group_attrs_by_line(attrs, source, &offset_to_line);
                    for group in &groups {
                        if group.len() > multiline_max {
                            if let Some(attr) = group.get(multiline_max) {
                                let name = attr_name(attr, source);
                                ctx.diagnostic(
                                    format!("'{}' should be on a new line.", name),
                                    attr_span(attr),
                                );
                            }
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

/// Group attributes by the line they start on.
/// Returns a Vec of groups (each group is a Vec of attribute refs), in source order.
fn group_attrs_by_line<'a, F>(
    attrs: &'a [Attribute],
    _source: &str,
    offset_to_line: &F,
) -> Vec<Vec<&'a Attribute>>
where
    F: Fn(usize) -> usize,
{
    // Mirrors the vendor's groupAttributesByLine logic:
    // attributes that share the same start line as the *end line* of the first attr
    // in the current group are placed in the same group.
    let mut groups: Vec<Vec<&Attribute>> = Vec::new();

    for attr in attrs {
        let attr_start_line = offset_to_line(attr_span(attr).start as usize);

        // Try to find an existing group whose first attr's end line matches this attr's start line.
        let found = if let Some(first_group) = groups.last_mut() {
            if let Some(first_attr) = first_group.first() {
                let first_end_line = offset_to_line(attr_span(first_attr).end as usize);
                first_end_line == attr_start_line
            } else {
                false
            }
        } else {
            false
        };

        if found {
            groups.last_mut().unwrap().push(attr);
        } else {
            groups.push(vec![attr]);
        }
    }

    groups
}
