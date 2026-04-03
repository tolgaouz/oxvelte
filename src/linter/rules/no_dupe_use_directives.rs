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
        use std::collections::HashMap;
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            let mut groups: HashMap<String, Vec<(String, oxc::span::Span)>> = HashMap::new();
            for attr in &el.attributes {
                if let Attribute::Directive { kind: DirectiveKind::Use, name, span, .. } = attr {
                    let text = &ctx.source[span.start as usize..span.end as usize];
                    let expr = text.find('=').map(|p| text[p + 1..].trim().to_string()).unwrap_or_default();
                    groups.entry(name.clone()).or_default().push((expr, *span));
                }
            }
            for (name, entries) in &groups {
                let mut by_expr: HashMap<&str, Vec<oxc::span::Span>> = HashMap::new();
                for (expr, span) in entries { by_expr.entry(expr.as_str()).or_default().push(*span); }
                for spans in by_expr.values().filter(|s| s.len() >= 2) {
                    for span in spans { ctx.diagnostic(format!("Duplicate use directive 'use:{}'.", name), *span); }
                }
            }
        });
    }
}
