//! `svelte/no-restricted-html-elements` — disallow specific HTML elements.

use crate::ast::TemplateNode;
use crate::linter::{walk_template_nodes, LintContext, Rule};
use oxc::span::Span;

pub struct NoRestrictedHtmlElements;

impl Rule for NoRestrictedHtmlElements {
    fn name(&self) -> &'static str {
        "svelte/no-restricted-html-elements"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Collect every (element-name, message) pair from the option list. Vendor
        // accepts either `string` or `{ elements: string[], message?: string }`;
        // unlike the previous implementation we no longer accept the
        // undocumented `element: string` shape.
        let mut restricted: Vec<(String, String)> = Vec::new();
        if let Some(arr) = ctx.config.options.as_ref().and_then(|o| o.as_array()) {
            for v in arr {
                match v {
                    serde_json::Value::String(s) => restricted.push((
                        s.clone(),
                        format!("Unexpected use of forbidden HTML element {}.", s),
                    )),
                    serde_json::Value::Object(obj) => {
                        let custom = obj.get("message").and_then(|m| m.as_str());
                        if let Some(els) = obj.get("elements").and_then(|e| e.as_array()) {
                            for el in els.iter().filter_map(|e| e.as_str()) {
                                let msg = custom.map(str::to_string).unwrap_or_else(|| {
                                    format!("Unexpected use of forbidden HTML element {}.", el)
                                });
                                restricted.push((el.to_string(), msg));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if restricted.is_empty() {
            return;
        }

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            if !el.kind().is_html() {
                return;
            }
            // Vendor compares element names case-sensitively.
            if let Some((_, msg)) = restricted.iter().find(|(e, _)| e == &el.name) {
                let start_tag = Span::new(el.span.start, el.start_tag_end);
                ctx.diagnostic(msg.clone(), start_tag);
            }
        });
    }
}
