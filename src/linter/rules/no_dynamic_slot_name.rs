//! `svelte/no-dynamic-slot-name` — disallow dynamic slot names.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};

pub struct NoDynamicSlotName;

impl Rule for NoDynamicSlotName {
    fn name(&self) -> &'static str {
        "svelte/no-dynamic-slot-name"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name == "slot" {
                    for attr in &el.attributes {
                        if let Attribute::NormalAttribute { name, value, span } = attr {
                            if name == "name" {
                                match value {
                                    AttributeValue::Static(_) => {
                                        // Static string is fine
                                    }
                                    AttributeValue::True => {
                                        ctx.diagnostic(
                                            "`<slot>` name requires a value.",
                                            *span,
                                        );
                                    }
                                    _ => {
                                        // Expression or Concat — dynamic
                                        ctx.diagnostic(
                                            "`<slot>` name cannot be dynamic.",
                                            *span,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
