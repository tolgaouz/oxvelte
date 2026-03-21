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
                // Skip special elements and components
                if el.name.starts_with("svelte:")
                    || el.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                {
                    return;
                }
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span } = attr {
                        if name == "class" {
                            match value {
                                AttributeValue::Expression(expr) => {
                                    if is_simple_class_ternary(expr) {
                                        ctx.diagnostic(
                                            "Consider using `class:name={condition}` instead of ternary in class attribute.",
                                            *span,
                                        );
                                    }
                                }
                                AttributeValue::Concat(parts) => {
                                    // Check each expression part independently.
                                    // Only flag ternaries that aren't concatenated with adjacent non-empty static text.
                                    for (i, part) in parts.iter().enumerate() {
                                        if let AttributeValuePart::Expression(expr) = part {
                                            if !is_simple_class_ternary(expr) {
                                                continue;
                                            }
                                            // Check adjacent parts — expression must be surrounded by
                                            // empty/whitespace static parts (not adjacent to other expressions)
                                            let prev_ok = if i > 0 {
                                                match &parts[i - 1] {
                                                    AttributeValuePart::Static(s) => s.is_empty() || s.ends_with(' '),
                                                    AttributeValuePart::Expression(_) => false,
                                                }
                                            } else { true };
                                            let next_ok = if i + 1 < parts.len() {
                                                match &parts[i + 1] {
                                                    AttributeValuePart::Static(s) => s.is_empty() || s.starts_with(' '),
                                                    AttributeValuePart::Expression(_) => false,
                                                }
                                            } else { true };

                                            if prev_ok && next_ok {
                                                ctx.diagnostic(
                                                    "Consider using `class:name={condition}` instead of ternary in class attribute.",
                                                    *span,
                                                );
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

/// Check if an expression is a simple ternary like `cond ? 'class-name' : ''`
fn is_simple_class_ternary(expr: &str) -> bool {
    let trimmed = expr.trim();
    if !trimmed.contains('?') || !trimmed.contains(':') {
        return false;
    }
    trimmed.ends_with(": ''")
        || trimmed.ends_with(": \"\"")
        || trimmed.starts_with("'' :")
        || trimmed.starts_with("\"\" :")
}
