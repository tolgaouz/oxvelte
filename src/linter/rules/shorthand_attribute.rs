//! `svelte/shorthand-attribute` — enforce use of shorthand syntax for attributes.
//! 🔧 Fixable

use crate::ast::{Attribute, AttributeValue, TemplateNode};
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
use oxc::ast::ast::Expression;

pub struct ShorthandAttribute;

impl Rule for ShorthandAttribute {
    fn name(&self) -> &'static str {
        "svelte/shorthand-attribute"
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
                if let Attribute::NormalAttribute {
                    name,
                    value: AttributeValue::Expression(expr),
                    span,
                } = attr
                {
                    let expression = el.attribute_meta.get(idx).and_then(|m| m.expression_ast);
                    if !expression_is_identifier(expression, expr, name) {
                        continue;
                    }
                    let src = &ctx.source[span.start as usize..span.end as usize];
                    if prefer_never && src.starts_with('{') {
                        let key = expression_identifier_name(expression).unwrap_or(name.as_str());
                        ctx.diagnostic_with_fix(
                            "Expected regular attribute syntax.",
                            *span,
                            Fix {
                                span: *span,
                                replacement: format!("{key}={{{key}}}"),
                            },
                        );
                    } else if !prefer_never && !src.starts_with('{') {
                        ctx.diagnostic_with_fix(
                            "Expected shorthand attribute.",
                            *span,
                            Fix {
                                span: *span,
                                replacement: format!("{{{name}}}"),
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
