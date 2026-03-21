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
                            if is_unoptimized(value) {
                                ctx.diagnostic(
                                    "Use `style:property={value}` directives instead of unoptimized style patterns for better performance.",
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

fn is_unoptimized(value: &AttributeValue) -> bool {
    match value {
        // style={dynamicExpr} — always unoptimized
        AttributeValue::Expression(_) => true,
        AttributeValue::Concat(parts) => {
            // Check for unoptimized patterns:
            // 1. No CSS declarations in static parts (whole style is expression)
            let static_text: String = parts.iter().filter_map(|p| {
                if let AttributeValuePart::Static(s) = p { Some(s.as_str()) } else { None }
            }).collect();

            if !static_text.contains(':') {
                return true;
            }

            // 2. CSS comments in static parts
            if static_text.contains("/*") {
                return true;
            }

            // 3. Check for expressions that replace whole declarations
            // (expression not between property: and ;)
            for (i, part) in parts.iter().enumerate() {
                if let AttributeValuePart::Expression(_) = part {
                    // Check what's before and after this expression
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

                    // Pattern: expression followed by `:` = dynamic property name
                    if let Some(a) = after {
                        let trimmed = a.trim_start();
                        if trimmed.starts_with(':') {
                            return true;
                        }
                    }

                    // Pattern: expression at a declaration boundary (after ; or start)
                    // but NOT after a property: (which would be a value interpolation)
                    if let Some(b) = before {
                        let trimmed = b.trim_end();
                        // If the last non-whitespace before the expression is ; or { or
                        // the expression is at position 0, it's replacing a whole declaration
                        if trimmed.ends_with(';') || trimmed.is_empty() {
                            // Check if next part is NOT a colon (which would be property: {value})
                            let next_is_value = after.map(|a| {
                                // If after has a semicolon, the expression was a value
                                a.trim_start().starts_with(';') || a.trim_start().starts_with('}')
                            }).unwrap_or(false);
                            if !next_is_value {
                                return true;
                            }
                        }
                    }
                }
            }

            false
        }
        _ => false,
    }
}
