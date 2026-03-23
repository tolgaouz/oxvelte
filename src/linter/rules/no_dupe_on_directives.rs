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
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Group event handlers by name, then check for duplicates with same expression text
                let mut groups: std::collections::HashMap<String, Vec<(String, oxc::span::Span)>> = std::collections::HashMap::new();
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::EventHandler, name, span, .. } = attr {
                        // Get the expression text from the source
                        let attr_text = &ctx.source[span.start as usize..span.end as usize];
                        // Extract the expression part (after the = sign)
                        let expr_text = attr_text.find('=')
                            .map(|pos| attr_text[pos + 1..].trim().to_string())
                            .unwrap_or_default();
                        groups.entry(name.clone()).or_default().push((expr_text, *span));
                    }
                }
                // Report duplicates within each group that have the same expression
                for (_name, entries) in &groups {
                    for i in 0..entries.len() {
                        for j in (i + 1)..entries.len() {
                            if entries[i].0 == entries[j].0 {
                                ctx.diagnostic(
                                    format!("Duplicate on directive 'on:{}'.", _name),
                                    entries[j].1,
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}
