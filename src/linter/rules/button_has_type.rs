//! `svelte/button-has-type` — disallow usage of button without an explicit type attribute.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};

pub struct ButtonHasType;

impl Rule for ButtonHasType {
    fn name(&self) -> &'static str {
        "svelte/button-has-type"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Config: { "button": false, "submit": false, "reset": false }
        // When a type value is set to false, that type is forbidden.
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .cloned();

        // Determine which type values are forbidden
        let button_forbidden = opts.as_ref()
            .and_then(|o| o.get("button"))
            .and_then(|v| v.as_bool())
            .map(|v| !v)
            .unwrap_or(false);
        let submit_forbidden = opts.as_ref()
            .and_then(|o| o.get("submit"))
            .and_then(|v| v.as_bool())
            .map(|v| !v)
            .unwrap_or(false);
        let reset_forbidden = opts.as_ref()
            .and_then(|o| o.get("reset"))
            .and_then(|v| v.as_bool())
            .map(|v| !v)
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "button" {
                    return;
                }

                let type_attr = el.attributes.iter().find(|attr| {
                    matches!(attr, Attribute::NormalAttribute { name, .. } if name == "type")
                });

                match type_attr {
                    None => {
                        ctx.diagnostic(
                            "Missing an explicit `type` attribute for `<button>`. Defaults to `\"submit\"` which may not be intended.",
                            el.span,
                        );
                    }
                    Some(Attribute::NormalAttribute { value, span, .. }) => {
                        let type_val = match value {
                            AttributeValue::Static(v) => Some(v.as_str()),
                            _ => None,
                        };
                        if let Some(val) = type_val {
                            let is_forbidden = match val {
                                "button" => button_forbidden,
                                "submit" => submit_forbidden,
                                "reset" => reset_forbidden,
                                _ => false,
                            };
                            if is_forbidden {
                                ctx.diagnostic(
                                    format!("{} is a forbidden value for button type attribute.", val),
                                    *span,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}
