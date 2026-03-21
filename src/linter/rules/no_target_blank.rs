//! `svelte/no-target-blank` — disallow `target="_blank"` without `rel="noopener noreferrer"`.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};

pub struct NoTargetBlank;

impl Rule for NoTargetBlank {
    fn name(&self) -> &'static str {
        "svelte/no-target-blank"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "a" {
                    return;
                }

                let has_target_blank = el.attributes.iter().any(|attr| {
                    matches!(
                        attr,
                        Attribute::NormalAttribute {
                            name,
                            value: AttributeValue::Static(v),
                            ..
                        } if name == "target" && v == "_blank"
                    )
                });

                if !has_target_blank {
                    return;
                }

                // Only flag external URLs (http, https, //)
                let href = el.attributes.iter().find_map(|attr| {
                    if let Attribute::NormalAttribute { name, value: AttributeValue::Static(v), .. } = attr {
                        if name == "href" { Some(v.as_str()) } else { None }
                    } else { None }
                });
                let is_external = href.map(|h| h.starts_with("http:") || h.starts_with("https:") || h.starts_with("//"))
                    .unwrap_or(false);
                let is_dynamic = el.attributes.iter().any(|attr| {
                    matches!(attr, Attribute::NormalAttribute { name, value: AttributeValue::Expression(_), .. } if name == "href")
                });
                if !is_external && !is_dynamic {
                    return;
                }

                let has_rel_noopener = el.attributes.iter().any(|attr| {
                    matches!(
                        attr,
                        Attribute::NormalAttribute {
                            name,
                            value: AttributeValue::Static(v),
                            ..
                        } if name == "rel" && v.contains("noopener") && v.contains("noreferrer")
                    )
                });

                if !has_rel_noopener {
                    ctx.diagnostic(
                        "Using `target=\"_blank\"` without `rel=\"noopener noreferrer\"` is a security risk.",
                        el.span,
                    );
                }
            }
        });
    }
}
