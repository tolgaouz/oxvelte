//! `svelte/no-dupe-style-properties` — disallow duplicate style properties.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, DirectiveKind, TemplateNode};
use rustc_hash::FxHashSet;

pub struct NoDupeStyleProperties;

impl Rule for NoDupeStyleProperties {
    fn name(&self) -> &'static str {
        "svelte/no-dupe-style-properties"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let mut seen_style_props: FxHashSet<String> = FxHashSet::default();

                for attr in &el.attributes {
                    if let Attribute::Directive {
                        kind: DirectiveKind::StyleDirective,
                        name,
                        span,
                        ..
                    } = attr
                    {
                        if !seen_style_props.insert(name.clone()) {
                            ctx.diagnostic(
                                format!("Duplicate style directive `style:{}`.", name),
                                *span,
                            );
                        }
                    }
                }
            }
        });
    }
}
