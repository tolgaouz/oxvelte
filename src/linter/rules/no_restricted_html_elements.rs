//! `svelte/no-restricted-html-elements` — disallow specific HTML elements.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoRestrictedHtmlElements;

impl Rule for NoRestrictedHtmlElements {
    fn name(&self) -> &'static str {
        "svelte/no-restricted-html-elements"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Get restricted elements from config options
        // Format can be: ["h1", "h2"] or [{ elements: ["h1"], message: "..." }]
        let mut restricted: Vec<(String, String)> = Vec::new(); // (element, message)
        if let Some(options) = &ctx.config.options {
            if let Some(arr) = options.as_array() {
                for v in arr {
                    match v {
                        serde_json::Value::String(s) => {
                            let msg = format!("Unexpected use of forbidden HTML element {}.", s);
                            restricted.push((s.clone(), msg));
                        }
                        serde_json::Value::Object(obj) => {
                            let msg = obj.get("message").and_then(|m| m.as_str())
                                .unwrap_or("This element is restricted.").to_string();
                            if let Some(elements) = obj.get("elements").and_then(|e| e.as_array()) {
                                for el in elements {
                                    if let Some(s) = el.as_str() {
                                        restricted.push((s.to_string(), msg.clone()));
                                    }
                                }
                            }
                            if let Some(el) = obj.get("element").and_then(|e| e.as_str()) {
                                restricted.push((el.to_string(), msg));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if restricted.is_empty() { return; }

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let lower = el.name.to_ascii_lowercase();
                for (elem, msg) in &restricted {
                    if elem == &lower {
                        ctx.diagnostic(msg.clone(), el.span);
                        break;
                    }
                }
            }
        });
    }
}
