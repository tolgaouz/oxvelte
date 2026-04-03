//! `svelte/no-target-blank` — disallow `target="_blank"` without `rel="noopener noreferrer"`.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};

pub struct NoTargetBlank;

impl Rule for NoTargetBlank {
    fn name(&self) -> &'static str {
        "svelte/no-target-blank"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref().and_then(|v| v.as_array()).and_then(|arr| arr.first());
        let enforce_dynamic = opts.and_then(|v| v.get("enforceDynamicLinks")).and_then(|v| v.as_str()).unwrap_or("always") == "always";
        let allow_referrer = opts.and_then(|v| v.get("allowReferrer")).and_then(|v| v.as_bool()).unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            if el.name != "a" { return; }
            let get_static = |n: &str| el.attributes.iter().find_map(|a| {
                if let Attribute::NormalAttribute { name, value: AttributeValue::Static(v), .. } = a {
                    if name == n { return Some(v.as_str()); }
                }
                None
            });
            if get_static("target") != Some("_blank") { return; }

            let is_external = get_static("href").map_or(false, |h| h.starts_with("http:") || h.starts_with("https:") || h.starts_with("//"));
            let is_dynamic = el.attributes.iter().any(|a| matches!(a, Attribute::NormalAttribute { name, value: AttributeValue::Expression(_), .. } if name == "href"));
            if !is_external && !is_dynamic { return; }
            if is_dynamic && !enforce_dynamic { return; }

            let has_safe_rel = get_static("rel").map_or(false, |v| {
                let t: Vec<&str> = v.split_whitespace().collect();
                t.contains(&"noopener") && (allow_referrer || t.contains(&"noreferrer"))
            });
            if !has_safe_rel {
                ctx.diagnostic("Using target=\"_blank\" without rel=\"noopener noreferrer\" is a security risk.", el.span);
            }
        });
    }
}
