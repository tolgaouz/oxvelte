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
                // Skip special elements and components (class: directives only work on HTML elements)
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
                                    // Whole attribute is a single expression
                                    if is_simple_class_ternary(expr) {
                                        ctx.diagnostic(
                                            "Consider using `class:name={condition}` instead of ternary in class attribute.",
                                            *span,
                                        );
                                    }
                                }
                                AttributeValue::Concat(parts) => {
                                    // Only flag if the ternary expression is the sole non-whitespace part
                                    // i.e., static parts are all empty or whitespace, and there's exactly one ternary expression
                                    let static_parts: Vec<&str> = parts.iter().filter_map(|p| {
                                        if let AttributeValuePart::Static(s) = p { Some(s.as_str()) } else { None }
                                    }).collect();
                                    let expr_parts: Vec<&str> = parts.iter().filter_map(|p| {
                                        if let AttributeValuePart::Expression(e) = p { Some(e.as_str()) } else { None }
                                    }).collect();

                                    // Only flag if all static parts are just spaces and there's one ternary
                                    if expr_parts.len() == 1
                                        && static_parts.iter().all(|s| s.trim().is_empty())
                                        && is_simple_class_ternary(expr_parts[0])
                                    {
                                        ctx.diagnostic(
                                            "Consider using `class:name={condition}` instead of ternary in class attribute.",
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

/// Check if an expression is a simple ternary like `cond ? 'class-name' : ''`
fn is_simple_class_ternary(expr: &str) -> bool {
    let trimmed = expr.trim();
    if !trimmed.contains('?') || !trimmed.contains(':') {
        return false;
    }
    // Simple check: ternary with empty string alternate
    trimmed.ends_with(": ''")
        || trimmed.ends_with(": \"\"")
        || trimmed.starts_with("'' :")
        || trimmed.starts_with("\"\" :")
}
