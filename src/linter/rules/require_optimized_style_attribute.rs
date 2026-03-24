//! `svelte/require-optimized-style-attribute` — require use of optimized style attribute syntax.
//!
//! Flags style attributes that use unoptimizable patterns:
//! - Entire style as a dynamic expression
//! - Expressions replacing whole CSS declarations
//! - Dynamic property names
//! - CSS comments preventing optimization

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, TemplateNode};

pub struct RequireOptimizedStyleAttribute;

impl Rule for RequireOptimizedStyleAttribute {
    fn name(&self) -> &'static str {
        "svelte/require-optimized-style-attribute"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, span } = attr {
                        if name == "style" {
                            if let Some(reason) = unoptimized_reason(value) {
                                ctx.diagnostic(reason, *span);
                            }
                        }
                    }
                }
            }
        });
    }
}

fn unoptimized_reason(value: &AttributeValue) -> Option<String> {
    match value {
        // style={dynamicExpr} — shorthand, always unoptimized
        AttributeValue::Expression(_) => {
            Some("It cannot be optimized because too complex.".to_string())
        }
        AttributeValue::Concat(parts) => {
            let static_text: String = parts.iter().filter_map(|p| {
                if let AttributeValuePart::Static(s) = p { Some(s.as_str()) } else { None }
            }).collect();

            // No CSS declarations in static parts (whole style is expression)
            if !static_text.contains(':') {
                return Some("It cannot be optimized because too complex.".to_string());
            }

            // CSS comments in static parts
            if static_text.contains("/*") {
                return Some("It cannot be optimized because contains comments.".to_string());
            }

            // Check for expressions that replace whole declarations or dynamic property names
            for (i, part) in parts.iter().enumerate() {
                if let AttributeValuePart::Expression(_) = part {
                    let before = if i > 0 {
                        if let AttributeValuePart::Static(s) = &parts[i - 1] {
                            Some(s.as_str())
                        } else { None }
                    } else { None };

                    let after = if i + 1 < parts.len() {
                        if let AttributeValuePart::Static(s) = &parts[i + 1] {
                            Some(s.as_str())
                        } else { None }
                    } else { None };

                    // Expression followed by `:` = dynamic property name
                    if let Some(a) = after {
                        if a.trim_start().starts_with(':') {
                            return Some("It cannot be optimized because property of style declaration contain interpolation.".to_string());
                        }
                    }

                    // Expression at a declaration boundary
                    if let Some(b) = before {
                        let trimmed = b.trim_end();
                        if trimmed.ends_with(';') || trimmed.is_empty() {
                            let next_is_value = after.map(|a| {
                                a.trim_start().starts_with(';') || a.trim_start().starts_with('}')
                            }).unwrap_or(false);
                            if !next_is_value {
                                return Some("It cannot be optimized because too complex.".to_string());
                            }
                        }
                    }
                }
            }

            None
        }
        _ => None,
    }
}
