//! `svelte/no-restricted-html-elements` — disallow specific HTML elements.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoRestrictedHtmlElements;

impl Rule for NoRestrictedHtmlElements {
    fn name(&self) -> &'static str {
        "svelte/no-restricted-html-elements"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let mut restricted: Vec<(String, String)> = Vec::new();
        if let Some(arr) = ctx.config.options.as_ref().and_then(|o| o.as_array()) {
            for v in arr {
                match v {
                    serde_json::Value::String(s) => restricted.push((s.clone(), format!("Unexpected use of forbidden HTML element {}.", s))),
                    serde_json::Value::Object(obj) => {
                        let msg = obj.get("message").and_then(|m| m.as_str()).unwrap_or("This element is restricted.").to_string();
                        if let Some(els) = obj.get("elements").and_then(|e| e.as_array()) {
                            for el in els.iter().filter_map(|e| e.as_str()) { restricted.push((el.to_string(), msg.clone())); }
                        }
                        if let Some(el) = obj.get("element").and_then(|e| e.as_str()) { restricted.push((el.to_string(), msg)); }
                    }
                    _ => {}
                }
            }
        }
        if restricted.is_empty() { return; }
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            let lower = el.name.to_ascii_lowercase();
            if let Some((_, msg)) = restricted.iter().find(|(e, _)| e == &lower) { ctx.diagnostic(msg.clone(), el.span); }
        });
    }
}
