//! `svelte/require-optimized-style-attribute` — require use of optimized style attribute syntax.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, TemplateNode};

const TOO_COMPLEX: &str = "It cannot be optimized because too complex.";

pub struct RequireOptimizedStyleAttribute;

impl Rule for RequireOptimizedStyleAttribute {
    fn name(&self) -> &'static str { "svelte/require-optimized-style-attribute" }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            for attr in &el.attributes {
                if let Attribute::NormalAttribute { name, value, span } = attr {
                    if name == "style" {
                        if let Some(reason) = unoptimized_reason(value) { ctx.diagnostic(reason, *span); }
                    }
                }
            }
        });
    }
}

fn get_static(part: &AttributeValuePart) -> Option<&str> {
    if let AttributeValuePart::Static(s) = part { Some(s.as_str()) } else { None }
}

fn unoptimized_reason(value: &AttributeValue) -> Option<&'static str> {
    match value {
        AttributeValue::Expression(_) => Some(TOO_COMPLEX),
        AttributeValue::Concat(parts) => {
            let static_text: String = parts.iter().filter_map(|p| get_static(p)).collect();
            if !static_text.contains(':') { return Some(TOO_COMPLEX); }
            if static_text.contains("/*") { return Some("It cannot be optimized because contains comments."); }

            for (i, part) in parts.iter().enumerate() {
                if !matches!(part, AttributeValuePart::Expression(_)) { continue; }
                let before = (i > 0).then(|| parts.get(i - 1)).flatten().and_then(|p| get_static(p));
                let after = parts.get(i + 1).and_then(|p| get_static(p));
                if after.map_or(false, |a| a.trim_start().starts_with(':')) {
                    return Some("It cannot be optimized because property of style declaration contain interpolation.");
                }
                if let Some(b) = before {
                    let t = b.trim_end();
                    if (t.ends_with(';') || t.is_empty())
                        && !after.map_or(false, |a| { let s = a.trim_start(); s.starts_with(';') || s.starts_with('}') }) {
                        return Some(TOO_COMPLEX);
                    }
                }
            }
            None
        }
        _ => None,
    }
}
