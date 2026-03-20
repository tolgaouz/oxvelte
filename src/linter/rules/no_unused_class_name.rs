//! `svelte/no-unused-class-name` — disallow class names in `<style>` that are not
//! used in the template markup.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};
use oxc::span::Span;

pub struct NoUnusedClassName;

impl Rule for NoUnusedClassName {
    fn name(&self) -> &'static str {
        "svelte/no-unused-class-name"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let style = match &ctx.ast.css {
            Some(s) => s,
            None => return,
        };

        // Collect class names used in the template.
        let mut used_classes: Vec<String> = Vec::new();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, value, .. } = attr {
                        if name == "class" {
                            if let AttributeValue::Static(val) = value {
                                for cls in val.split_whitespace() {
                                    used_classes.push(cls.to_string());
                                }
                            }
                        }
                    }
                    if let Attribute::Directive { kind: crate::ast::DirectiveKind::Class, name: cls_name, .. } = attr {
                        used_classes.push(cls_name.clone());
                    }
                }
            }
        });

        // Extract class selectors from CSS content (simple heuristic).
        let css = &style.content;
        let base = style.span.start as usize;
        let mut i = 0;
        let bytes = css.as_bytes();
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
                    if !used_classes.iter().any(|c| c == class_name) {
                        let span_start = (base + i) as u32;
                        let span_end = (base + end) as u32;
                        ctx.diagnostic(
                            format!("Class `.{}` is defined in `<style>` but not used in the template.", class_name),
                            Span::new(span_start, span_end),
                        );
                    }
                }
                i = end;
            } else {
                i += 1;
            }
        }
    }
}
