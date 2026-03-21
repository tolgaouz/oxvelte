//! `svelte/no-unused-class-name` — disallow class names in the template that are not
//! defined in the `<style>` block.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, DirectiveKind};
use std::collections::HashSet;

pub struct NoUnusedClassName;

impl Rule for NoUnusedClassName {
    fn name(&self) -> &'static str {
        "svelte/no-unused-class-name"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Step 1: Extract all class selectors from the CSS content
        let mut css_classes = HashSet::new();
        if let Some(style) = &ctx.ast.css {
            let css = &style.content;
            let bytes = css.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'.' {
                    let start = i + 1;
                    let mut end = start;
                    while end < bytes.len()
                        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b'_')
                    {
                        end += 1;
                    }
                    if end > start {
                        let class_name = &css[start..end];
                        css_classes.insert(class_name.to_string());
                    }
                    i = end;
                } else {
                    i += 1;
                }
            }
        }

        // Step 2: Collect all template classes and check if they're defined in CSS
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let mut element_classes = Vec::new();

                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name, value, .. } if name == "class" => {
                            if let AttributeValue::Static(val) = value {
                                for cls in val.split_whitespace() {
                                    if !cls.is_empty() {
                                        element_classes.push(cls.to_string());
                                    }
                                }
                            }
                        }
                        Attribute::Directive { kind: DirectiveKind::Class, name: cls_name, .. } => {
                            element_classes.push(cls_name.clone());
                        }
                        _ => {}
                    }
                }

                // Report template classes not found in CSS
                for cls in &element_classes {
                    if !css_classes.contains(cls.as_str()) {
                        ctx.diagnostic(
                            format!("Unused class \"{}\".", cls),
                            el.span,
                        );
                    }
                }
            }
        });
    }
}
