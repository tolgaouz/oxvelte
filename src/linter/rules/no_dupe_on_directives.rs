//! `svelte/no-dupe-on-directives` — disallow duplicate on directives.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct NoDupeOnDirectives;

impl Rule for NoDupeOnDirectives {
    fn name(&self) -> &'static str {
        "svelte/no-dupe-on-directives"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        use std::collections::HashMap;
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            let mut groups: HashMap<String, Vec<(String, oxc::span::Span)>> = HashMap::new();
            for attr in &el.attributes {
                if let Attribute::Directive { kind: DirectiveKind::EventHandler, name, span, .. } = attr {
                    let text = &ctx.source[span.start as usize..span.end as usize];
                    let expr = text.find('=').map(|p| text[p + 1..].trim().to_string()).unwrap_or_default();
                    groups.entry(name.clone()).or_default().push((expr, *span));
                }
            }
            for (name, entries) in &groups {
                let mut by_expr: HashMap<&str, Vec<oxc::span::Span>> = HashMap::new();
                for (expr, span) in entries { by_expr.entry(expr.as_str()).or_default().push(*span); }
                for spans in by_expr.values().filter(|s| s.len() >= 2) {
                    for span in spans { ctx.diagnostic(format!("Duplicate on directive 'on:{}'.", name), *span); }
                }
            }
        });
    }
}
