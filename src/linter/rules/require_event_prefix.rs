//! `svelte/require-event-prefix` — require event handler directives to use the `on` prefix.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct RequireEventPrefix;

impl Rule for RequireEventPrefix {
    fn name(&self) -> &'static str {
        "svelte/require-event-prefix"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive {
                        kind: DirectiveKind::EventHandler,
                        name: handler_name,
                        span,
                        ..
                    } = attr
                    {
                        if !handler_name.starts_with("on") {
                            ctx.diagnostic(
                                format!(
                                    "Event handler `{}` should use the `on` prefix (e.g. `on:{}`).",
                                    handler_name, handler_name
                                ),
                                *span,
                            );
                        }
                    }
                }
            }
        });
    }
}
