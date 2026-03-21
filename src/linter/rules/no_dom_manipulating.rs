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
        // Step 1: Find all bind:this variables from the template
        let mut bound_vars = HashSet::new();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Only track bind:this on native HTML elements, not components
                // Components start with uppercase. svelte:element is native, but
                // svelte:component and svelte:self are not.
                let is_native = el.name == "svelte:element"
                    || (el.name.chars().next().map_or(false, |c| c.is_ascii_lowercase())
                        && !el.name.starts_with("svelte:component")
                        && !el.name.starts_with("svelte:self"));
                if !is_native { return; }
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::Binding, name, span, .. } = attr {
                        if name == "this" {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            if let Some(open) = region.find('{') {
                                if let Some(close) = region.find('}') {
                                    let var = region[open+1..close].trim();
                                    if !var.is_empty() && var.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                                        bound_vars.insert(var.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let source = ctx.source;
            let tag_text = &source[script.span.start as usize..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| script.span.start as usize + p + 1).unwrap_or(script.span.start as usize);

            // Only check variables that are explicitly declared in the script
            let declared_bound: HashSet<_> = bound_vars.iter()
                .filter(|var| {
                    let decl_patterns = [
                        format!("let {};", var),
                        format!("let {}", var),
                        format!("let {},", var),
                        format!("let {}\n", var),
                    ];
                    decl_patterns.iter().any(|p| content.contains(p.as_str()))
                })
                .collect();

            for var in &declared_bound {
                // Check DOM method calls: var.method(, var?.method(, (var?.method)(
                for method in DOM_METHODS {
                    let patterns = [
                        format!("{}.{}(", var, method),
                        format!("{}?.{}(", var, method),
                        format!("({}?.{})(", var, method),
                    ];
                    for pattern in &patterns {
                        let mut search_from = 0;
                        while let Some(pos) = content[search_from..].find(pattern.as_str()) {
                            let abs = search_from + pos;
                            let source_pos = content_offset + abs;
                            ctx.diagnostic(
                                "Do not mutate the DOM directly. Use Svelte's reactivity instead.",
                                oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                            );
                            search_from = abs + pattern.len();
                        }
                    }
                }

                // Check DOM property assignments: var.prop =
                for prop in DOM_PROPS {
                    let patterns = [
                        format!("{}.{}", var, prop),
                        format!("{}?.{}", var, prop),
                    ];
                    for pattern in &patterns {
                        let mut search_from = 0;
                        while let Some(pos) = content[search_from..].find(pattern.as_str()) {
                            let abs = search_from + pos;
                            let after = content[abs + pattern.len()..].trim_start();
                            if after.starts_with('=') && !after.starts_with("==") {
                                let source_pos = content_offset + abs;
                                ctx.diagnostic(
                                    "Do not mutate the DOM directly. Use Svelte's reactivity instead.",
                                    oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                                );
                            }
                            search_from = abs + pattern.len();
                        }
                    }
                }
            }
        }
    }
}
