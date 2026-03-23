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
        // Config: { "prefer": "always" | "never" }, default "always"
        let prefer_never = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("prefer"))
            .and_then(|v| v.as_str())
            .map(|s| s == "never")
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind, name, span, .. } = attr {
                        let directive_prefix = match kind {
                            DirectiveKind::Binding => "bind",
                            DirectiveKind::Class => "class",
                            DirectiveKind::Let => "let",
                            _ => continue,
                        };

                        let region = &ctx.source[span.start as usize..span.end as usize];

                        if prefer_never {
                            // "never" mode: flag shorthand usage (no '=' present means shorthand)
                            if !region.contains('=') {
                                ctx.diagnostic(
                                    "Expected regular directive syntax.",
                                    *span,
                                );
                            }
                        } else {
                            // "always" mode (default): flag longhand when shorthand is possible
                            if let Some(eq_pos) = region.find('=') {
                                let value_part = region[eq_pos + 1..].trim();
                                let expr = if value_part.starts_with('{') && value_part.ends_with('}') {
                                    &value_part[1..value_part.len()-1]
                                } else {
                                    value_part
                                };
                                if expr.trim() == name.as_str() {
                                    ctx.diagnostic(
                                        format!("Use shorthand `{}:{}` instead of `{}:{}={{{}}}`.", directive_prefix, name, directive_prefix, name, name),
                                        *span,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
