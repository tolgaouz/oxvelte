//! `svelte/no-inline-styles` — disallow inline styles on elements.

use crate::ast::{Attribute, DirectiveKind, TemplateNode};
use crate::linter::{walk_template_nodes, LintContext, Rule};

pub struct NoInlineStyles;

impl Rule for NoInlineStyles {
    fn name(&self) -> &'static str {
        "svelte/no-inline-styles"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let allow_transitions = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|o| o.get("allowTransitions"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            // The rule targets DOM-rendering elements. Components have their
            // own style-passing API; `<svelte:options>` / `<svelte:head>` etc.
            // can carry attributes that aren't really "inline styles". The
            // one Svelte special that does render a DOM node is
            // `<svelte:element>`.
            match el.kind() {
                crate::ast::ElementKind::Html => {}
                crate::ast::ElementKind::SvelteSpecial(crate::ast::SvelteSpecial::Element) => {}
                _ => return,
            }
            for attr in &el.attributes {
                match attr {
                    Attribute::NormalAttribute { name, span, .. } if name == "style" => {
                        ctx.diagnostic("Found disallowed style attribute.", *span)
                    }
                    Attribute::Directive {
                        kind: DirectiveKind::StyleDirective,
                        span,
                        ..
                    } => ctx.diagnostic("Found disallowed style directive.", *span),
                    Attribute::Directive {
                        kind: DirectiveKind::Transition,
                        span,
                        ..
                    } if !allow_transitions => {
                        ctx.diagnostic("Found disallowed transition.", *span)
                    }
                    _ => {}
                }
            }
        });
    }
}
