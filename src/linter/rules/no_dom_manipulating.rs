//! `svelte/no-dom-manipulating` — disallow DOM manipulating.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use std::collections::HashSet;

const DOM_METHODS: &[&str] = &[
    "appendChild", "removeChild", "insertBefore", "replaceChild",
    "normalize", "after", "append", "before",
    "insertAdjacentElement", "insertAdjacentHTML", "insertAdjacentText",
    "prepend", "remove", "replaceChildren", "replaceWith",
];

const DOM_PROPS: &[&str] = &[
    "textContent", "innerHTML", "outerHTML", "innerText", "outerText",
];

pub struct NoDomManipulating;

impl Rule for NoDomManipulating {
    fn name(&self) -> &'static str {
        "svelte/no-dom-manipulating"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let mut bound_vars = HashSet::new();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            let is_native = el.name == "svelte:element"
                || (el.name.as_bytes().first().map_or(false, |c| c.is_ascii_lowercase())
                    && !el.name.starts_with("svelte:component") && !el.name.starts_with("svelte:self"));
            if !is_native { return; }
            for attr in &el.attributes {
                if let Attribute::Directive { kind: DirectiveKind::Binding, name, span, .. } = attr {
                    if name != "this" { continue; }
                    let region = &ctx.source[span.start as usize..span.end as usize];
                    if let (Some(open), Some(close)) = (region.find('{'), region.find('}')) {
                        let var = region[open+1..close].trim();
                        if !var.is_empty() && var.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                            bound_vars.insert(var.to_string());
                        }
                    }
                }
            }
        });

        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| script.span.start as usize + p + 1).unwrap_or(script.span.start as usize);

        for var in bound_vars.iter().filter(|v| content.contains(&format!("let {}", v))) {
            let prefixes = [format!("{}.", var), format!("{}?.", var), format!("({}?.", var)];
            for prefix in &prefixes {
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(prefix.as_str()) {
                    let abs = search_from + pos;
                    let after = &content[abs + prefix.len()..];
                    let is_method = DOM_METHODS.iter().any(|m| after.starts_with(m) && after.as_bytes().get(m.len()) == Some(&b'('));
                    let is_prop = DOM_PROPS.iter().any(|p| {
                        after.starts_with(p) && { let r = after[p.len()..].trim_start(); r.starts_with('=') && !r.starts_with("==") }
                    });
                    if is_method || is_prop {
                        let source_pos = content_offset + abs;
                        let end = source_pos + prefix.len() + after.find('(').or_else(|| after.find('=')).unwrap_or(4);
                        ctx.diagnostic("Don't manipulate the DOM directly. The Svelte runtime can get confused if there is a difference between the actual DOM and the DOM expected by the Svelte runtime.",
                            oxc::span::Span::new(source_pos as u32, end as u32));
                    }
                    search_from = abs + prefix.len();
                }
            }
        }
    }
}
