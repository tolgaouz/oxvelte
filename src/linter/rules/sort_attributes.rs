//! `svelte/sort-attributes` — enforce attribute sorting order.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute};

pub struct SortAttributes;

impl Rule for SortAttributes {
    fn name(&self) -> &'static str {
        "svelte/sort-attributes"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let names: Vec<&str> = el
                    .attributes
                    .iter()
                    .filter_map(|a| match a {
                        Attribute::NormalAttribute { name, .. } => Some(name.as_str()),
                        Attribute::Directive { name, .. } => Some(name.as_str()),
                        _ => None,
                    })
                    .collect();
                // Check if already sorted alphabetically.
                for window in names.windows(2) {
                    if window[0].to_lowercase() > window[1].to_lowercase() {
                        ctx.diagnostic(
                            format!(
                                "Attributes should be sorted alphabetically. `{}` should come before `{}`.",
                                window[1], window[0]
                            ),
                            el.span,
                        );
                        break;
                    }
                }
            }
        });
    }
}
