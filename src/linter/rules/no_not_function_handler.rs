//! `svelte/no-not-function-handler` — disallow non-function event handlers.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct NoNotFunctionHandler;

impl Rule for NoNotFunctionHandler {
    fn name(&self) -> &'static str {
        "svelte/no-not-function-handler"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } = attr {
                        // Check source text to see if the handler value looks like a non-function
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(eq_pos) = region.find('=') {
                            let value = region[eq_pos + 1..].trim();
                            if value.starts_with('{') && value.ends_with('}') {
                                let expr = &value[1..value.len()-1].trim();
                                // Check for literal values that are clearly not functions
                                // null is valid (used to conditionally disable handlers)
                                if *expr == "true" || *expr == "false"
                                    || *expr == "undefined"
                                    || expr.parse::<f64>().is_ok()
                                    || (expr.starts_with('"') && expr.ends_with('"'))
                                    || (expr.starts_with('\'') && expr.ends_with('\''))
                                    || (expr.starts_with('[') && expr.ends_with(']'))
                                    || (expr.starts_with('{') && expr.ends_with('}'))
                                    || expr.starts_with("class ")
                                    || expr.starts_with("new ")
                                {
                                    ctx.diagnostic(
                                        format!("Expected a function as event handler, got '{}'.", expr),
                                        *span,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
