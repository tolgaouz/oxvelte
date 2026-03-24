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
                let mut groups: std::collections::HashMap<String, Vec<(String, oxc::span::Span)>> = std::collections::HashMap::new();
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::Use, name, span, .. } = attr {
                        let attr_text = &ctx.source[span.start as usize..span.end as usize];
                        let expr_text = attr_text.find('=')
                            .map(|pos| attr_text[pos + 1..].trim().to_string())
                            .unwrap_or_default();
                        groups.entry(name.clone()).or_default().push((expr_text, *span));
                    }
                }
                // Report ALL members of any duplicate group
                for (_name, entries) in &groups {
                    let mut expr_groups: std::collections::HashMap<&str, Vec<oxc::span::Span>> = std::collections::HashMap::new();
                    for (expr, span) in entries {
                        expr_groups.entry(expr.as_str()).or_default().push(*span);
                    }
                    for (_expr, spans) in &expr_groups {
                        if spans.len() >= 2 {
                            for span in spans {
                                ctx.diagnostic(
                                    format!("Duplicate use directive 'use:{}'.", _name),
                                    *span,
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}
