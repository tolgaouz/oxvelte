//! `svelte/shorthand-directive` — enforce use of shorthand syntax for directives.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct ShorthandDirective;

impl Rule for ShorthandDirective {
    fn name(&self) -> &'static str {
        "svelte/shorthand-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind, name, span, .. } = attr {
                        // Check if the directive value is the same as the name
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(eq_pos) = region.find('=') {
                            let value_part = region[eq_pos + 1..].trim();
                            let expr = if value_part.starts_with('{') && value_part.ends_with('}') {
                                &value_part[1..value_part.len()-1]
                            } else {
                                value_part
                            };
                            if expr.trim() == name.as_str() {
                                let directive_prefix = match kind {
                                    DirectiveKind::Binding => "bind",
                                    DirectiveKind::Class => "class",
                                    DirectiveKind::Let => "let",
                                    _ => continue,
                                };
                                ctx.diagnostic(
                                    format!("Use shorthand `{}:{}` instead of `{}:{}={{{}}}`.", directive_prefix, name, directive_prefix, name, name),
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
