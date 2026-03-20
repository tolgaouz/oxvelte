//! `svelte/button-has-type` — disallow usage of button without an explicit type attribute.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, TemplateNode};

pub struct ButtonHasType;

impl Rule for ButtonHasType {
    fn name(&self) -> &'static str {
        "svelte/button-has-type"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "button" {
                    return;
                }

                let has_type = el.attributes.iter().any(|attr| {
                    matches!(attr, Attribute::NormalAttribute { name, .. } if name == "type")
                });

                if !has_type {
                    ctx.diagnostic(
                        "Missing an explicit `type` attribute for `<button>`. Defaults to `\"submit\"` which may not be intended.",
                        el.span,
                    );
                }
            }
        });
    }
}
