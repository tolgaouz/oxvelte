//! `svelte/button-has-type` — disallow usage of button without an explicit type attribute.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, DirectiveKind, TemplateNode};

pub struct ButtonHasType;

impl Rule for ButtonHasType {
    fn name(&self) -> &'static str {
        "svelte/button-has-type"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .cloned();

        let is_forbidden = |key: &str| opts.as_ref()
            .and_then(|o| o.get(key)).and_then(|v| v.as_bool())
            .map(|v| !v).unwrap_or(false);
        let button_forbidden = is_forbidden("button");
        let submit_forbidden = is_forbidden("submit");
        let reset_forbidden = is_forbidden("reset");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "button" {
                    return;
                }

                let has_bind_type = el.attributes.iter().any(|attr| {
                    matches!(attr, Attribute::Directive { kind: DirectiveKind::Binding, name, .. } if name == "type")
                });
                if has_bind_type {
                    return;
                }

                let has_shorthand_type = el.attributes.iter().any(|attr| {
                    if let Attribute::NormalAttribute { name, value, span, .. } = attr {
                        if name == "type" {
                            if let AttributeValue::Expression(_) = value {
                                let src = &ctx.source[span.start as usize..span.end as usize];
                                return src.starts_with('{');
                            }
                        }
                    }
                    false
                });
                if has_shorthand_type {
                    return;
                }

                let type_attr = el.attributes.iter().find(|attr| {
                    matches!(attr, Attribute::NormalAttribute { name, .. } if name == "type")
                });

                match type_attr {
                    Some(Attribute::NormalAttribute { value, span, .. }) => {
                        match value {
                            AttributeValue::True => {
                                ctx.diagnostic("A value must be set for button type attribute.", *span);
                            }
                            AttributeValue::Static(v) => {
                                if v.is_empty() {
                                    ctx.diagnostic("A value must be set for button type attribute.", *span);
                                } else if !matches!(v.as_str(), "button" | "submit" | "reset") {
                                    ctx.diagnostic(format!("{} is an invalid value for button type attribute.", v), *span);
                                } else {
                                    let forbidden = match v.as_str() {
                                        "button" => button_forbidden, "submit" => submit_forbidden,
                                        "reset" => reset_forbidden, _ => false,
                                    };
                                    if forbidden { ctx.diagnostic(format!("{} is a forbidden value for button type attribute.", v), *span); }
                                }
                            }
                            _ => {}
                        }
                    }
                    None => {
                        if el.attributes.iter().any(|a| matches!(a, Attribute::Spread { .. })) { return; }
                        ctx.diagnostic("Missing an explicit type attribute for button.",
                            el.span);
                    }
                    _ => {}
                }
            }
        });
    }
}
