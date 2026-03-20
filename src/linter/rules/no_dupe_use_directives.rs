//! `svelte/no-dupe-use-directives` — disallow duplicate use directives.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct NoDupeUseDirectives;

impl Rule for NoDupeUseDirectives {
    fn name(&self) -> &'static str {
        "svelte/no-dupe-use-directives"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let mut seen = std::collections::HashSet::new();
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::Use, name, span, .. } = attr {
                        if !seen.insert(name.as_str()) {
                            ctx.diagnostic(
                                format!("Duplicate use directive 'use:{}'.", name),
                                *span,
                            );
                        }
                    }
                }
            }
        });
    }
}
