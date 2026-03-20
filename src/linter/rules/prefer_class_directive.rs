//! `svelte/prefer-class-directive` — prefer class directives over ternary class attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart};

pub struct PreferClassDirective;

impl Rule for PreferClassDirective {
    fn name(&self) -> &'static str {
        "svelte/prefer-class-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span } = attr {
                        if name == "class" {
                            match value {
                                AttributeValue::Concat(parts) => {
                                    // Check if any expression part looks like a ternary for class toggling
                                    for part in parts {
                                        if let AttributeValuePart::Expression(expr) = part {
                                            if expr.contains('?') && expr.contains(':') {
                                                let trimmed = expr.trim();
                                                // Simple heuristic: if ternary has empty string alternate
                                                if trimmed.ends_with(": ''") || trimmed.ends_with(": \"\"") {
                                                    ctx.diagnostic(
                                                        "Consider using `class:name={condition}` instead of ternary in class attribute.",
                                                        *span,
                                                    );
                                                }
                                            }
                                        }
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
