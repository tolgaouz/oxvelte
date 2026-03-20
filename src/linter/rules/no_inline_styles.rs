//! `svelte/no-inline-styles` — disallow inline styles on elements.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct NoInlineStyles;

impl Rule for NoInlineStyles {
    fn name(&self) -> &'static str {
        "svelte/no-inline-styles"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name, span, .. } if name == "style" => {
                            ctx.diagnostic("Avoid inline styles.", *span);
                        }
                        Attribute::Directive { kind: DirectiveKind::StyleDirective, span, .. } => {
                            ctx.diagnostic("Avoid inline styles.", *span);
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}
