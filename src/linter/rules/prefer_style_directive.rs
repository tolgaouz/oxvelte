//! `svelte/prefer-style-directive` — prefer `style:` directives over `style` attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};

pub struct PreferStyleDirective;

impl Rule for PreferStyleDirective {
    fn name(&self) -> &'static str {
        "svelte/prefer-style-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Skip components (style: directives only work on HTML/svelte:element)
                // svelte:element renders as a real DOM element, so it supports style:
                if el.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                    || (el.name.starts_with("svelte:") && el.name != "svelte:element")
                {
                    return;
                }
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span } = attr {
                        if name == "style" {
                            // Only flag static style attributes with parseable CSS declarations
                            // Don't flag expression-only styles (variables), shorthand, or concat
                            match value {
                                AttributeValue::Static(s) => {
                                    // Report once per CSS declaration
                                    let decl_count = s.split(';')
                                        .filter(|d| d.trim().contains(':'))
                                        .count();
                                    for _ in 0..decl_count {
                                        ctx.diagnostic(
                                            "Can use style directives instead.",
                                            *span,
                                        );
                                    }
                                }
                                AttributeValue::Concat(parts) => {
                                    // Flag if static parts contain CSS declarations or if
                                    // expressions contain CSS-like strings
                                    let has_css_pattern = parts.iter().any(|p| {
                                        match p {
                                            crate::ast::AttributeValuePart::Static(s) => s.contains(':'),
                                            crate::ast::AttributeValuePart::Expression(e) => {
                                                // Check if expression contains CSS-like property:value patterns
                                                e.contains(':')
                                            }
                                        }
                                    });
                                    if has_css_pattern {
                                        ctx.diagnostic(
                                            "Can use style directives instead.",
                                            *span,
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        });
    }
}
