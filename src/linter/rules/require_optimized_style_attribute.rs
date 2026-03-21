//! `svelte/require-optimized-style-attribute` — require use of optimized style attribute syntax.
//!
//! Flags style attributes that use a single dynamic expression for the entire style,
//! like `style={dynamicStyle}` or style="{transformStyle}". Individual property value
//! interpolation like `style="color: {color}"` is allowed.

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
                            match value {
                                // style={dynamicExpr} — always flag
                                AttributeValue::Expression(_) => {
                                    ctx.diagnostic(
                                        "Use `style:property={value}` directives instead of a dynamic style expression for better performance.",
                                        *span,
                                    );
                                }
                                // style="..." with concat — only flag if the expression
                                // replaces the entire style (not individual CSS values)
                                AttributeValue::Concat(parts) => {
                                    // If the concat is mostly expression (e.g., just "{transformStyle}")
                                    // with no CSS declarations in static parts, flag it
                                    let static_text: String = parts.iter().filter_map(|p| {
                                        if let AttributeValuePart::Static(s) = p { Some(s.as_str()) } else { None }
                                    }).collect();
                                    // If static parts have no CSS property:value pattern,
                                    // the expression likely sets the entire style
                                    if !static_text.contains(':') {
                                        ctx.diagnostic(
                                            "Use `style:property={value}` directives instead of concatenated style strings for better performance.",
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
