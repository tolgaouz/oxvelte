//! `svelte/shorthand-directive` — enforce use of shorthand syntax for directives.
//! 🔧 Fixable

use crate::ast::{Attribute, AttributeValue, DirectiveKind, TemplateNode};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use oxc::ast::ast::Expression;
use oxc::span::Span;

pub struct ShorthandDirective;

impl Rule for ShorthandDirective {
    fn name(&self) -> &'static str {
        "svelte/shorthand-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let prefer_never = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("prefer"))
            .and_then(|v| v.as_str())
            == Some("never");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            for (idx, attr) in el.attributes.iter().enumerate() {
                let Attribute::Directive {
                    kind,
                    name,
                    value,
                    span,
                    ..
                } = attr
                else {
                    continue;
                };
                if !matches!(
                    kind,
                    DirectiveKind::Binding | DirectiveKind::Class | DirectiveKind::StyleDirective
                ) {
                    continue;
                }
                let region = &ctx.source[span.start as usize..span.end as usize];
                if prefer_never {
                    if !region.contains('=') {
                        let insert_at = el
                            .attribute_meta
                            .get(idx)
                            .and_then(|m| m.directive_subject_span)
                            .unwrap_or_else(|| Span::new(span.end, span.end))
                            .end;
                        ctx.diagnostic_with_fix(
                            "Expected regular directive syntax.",
                            *span,
                            Fix {
                                span: Span::new(insert_at, insert_at),
                                replacement: format!("={{{name}}}"),
                            },
                        );
                    }
                } else if let Some(eq) = region.find('=') {
                    let expression = el.attribute_meta.get(idx).and_then(|m| m.expression_ast);
                    let raw = match value {
                        AttributeValue::Expression(expr) => expr.as_str(),
                        _ => continue,
                    };
                    if expression_is_identifier(expression, raw, name) {
                        let fix_start = span.start + eq as u32;
                        ctx.diagnostic_with_fix(
                            "Expected shorthand directive.",
                            *span,
                            Fix {
                                span: Span::new(fix_start, span.end),
                                replacement: String::new(),
                            },
                        );
                    }
                }
            }
        });
    }
}

fn expression_identifier_name<'a>(expr: Option<&'a Expression<'a>>) -> Option<&'a str> {
    match expr {
        Some(Expression::Identifier(id)) => Some(id.name.as_str()),
        _ => None,
    }
}

fn expression_is_identifier(expr: Option<&Expression>, raw: &str, expected: &str) -> bool {
    expression_identifier_name(expr).map_or_else(|| raw.trim() == expected, |name| name == expected)
}
