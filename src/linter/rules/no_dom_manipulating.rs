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

            // Only check variables that are explicitly declared with `let` in the script
            let declared_bound: HashSet<_> = bound_vars.iter()
                .filter(|var| {
                    // Check if `let VAR` appears as a declaration (followed by ;, space, comma, or newline)
                    let prefix = format!("let {}", var);
                    content.contains(&prefix)
                })
                .collect();

            for var in &declared_bound {
                let var_dot = format!("{}.", var);
                let var_opt = format!("{}?.", var);
                let var_paren_opt = format!("({}?.", var);

                // Check all occurrences of var. and var?. in content
                for prefix in &[&var_dot, &var_opt, &var_paren_opt] {
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(prefix.as_str()) {
                        let abs = search_from + pos;
                        let after = &content[abs + prefix.len()..];

                        // Check if it's a DOM method call (exact match followed by `(`)
                        let is_method = DOM_METHODS.iter().any(|m| {
                            after.starts_with(m)
                                && after.as_bytes().get(m.len()) == Some(&b'(')
                        });
                        // Check if it's a DOM property assignment
                        let is_prop_assign = DOM_PROPS.iter().any(|p| {
                            if !after.starts_with(p) { return false; }
                            let rest = after[p.len()..].trim_start();
                            rest.starts_with('=') && !rest.starts_with("==")
                        });

                        if is_method || is_prop_assign {
                            let source_pos = content_offset + abs;
                            let end = source_pos + prefix.len() + after.find('(').or_else(|| after.find('=')).unwrap_or(4);
                            ctx.diagnostic(
                                "Don't manipulate the DOM directly. The Svelte runtime can get confused if there is a difference between the actual DOM and the DOM expected by the Svelte runtime.",
                                oxc::span::Span::new(source_pos as u32, end as u32),
                            );
                        }
                        search_from = abs + prefix.len();
                    }
                }
            }
        }
    }
}
