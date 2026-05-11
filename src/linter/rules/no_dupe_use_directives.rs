//! `svelte/no-dupe-use-directives` — disallow duplicate use directives.
//! ⭐ Recommended

use crate::ast::{Attribute, DirectiveKind, TemplateNode};
use crate::linter::rules::directive_expression_key;
use crate::linter::{walk_template_nodes, LintContext, Rule};

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
            let TemplateNode::Element(el) = node else {
                return;
            };
            let mut groups: HashMap<String, Vec<(String, oxc::span::Span)>> = HashMap::new();
            for attr in &el.attributes {
                if let Attribute::Directive {
                    kind: DirectiveKind::Use,
                    name,
                    value,
                    span,
                    ..
                } = attr
                {
                    let expr = directive_expression_key(value);
                    groups.entry(name.clone()).or_default().push((expr, *span));
                }
            }
            for (name, entries) in &groups {
                let mut by_expr: HashMap<&str, Vec<oxc::span::Span>> = HashMap::new();
                for (expr, span) in entries {
                    by_expr.entry(expr.as_str()).or_default().push(*span);
                }
                for spans in by_expr.values().filter(|s| s.len() >= 2) {
                    for span in spans {
                        ctx.diagnostic(format!("Duplicate use directive 'use:{}'.", name), *span);
                    }
                }
            }
        });
    }
}
