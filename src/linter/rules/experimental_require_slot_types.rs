//! `svelte/experimental-require-slot-types` — require slot types to be defined
//! for components that expose slots.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct ExperimentalRequireSlotTypes;

fn is_ts_lang(lang: Option<&str>) -> bool {
    match lang {
        Some(l) => l.eq_ignore_ascii_case("ts") || l.eq_ignore_ascii_case("typescript"),
        None => false,
    }
}

impl Rule for ExperimentalRequireSlotTypes {
    fn name(&self) -> &'static str {
        "svelte/experimental-require-slot-types"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Only applies to TypeScript scripts (instance or module).
        let instance_is_ts = ctx.ast.instance.as_ref()
            .map(|s| is_ts_lang(s.lang.as_deref()))
            .unwrap_or(false);
        let module_is_ts = ctx.ast.module.as_ref()
            .map(|s| is_ts_lang(s.lang.as_deref()))
            .unwrap_or(false);
        if !instance_is_ts && !module_is_ts {
            return;
        }

        // Collect the first <slot> element span.
        let mut first_slot_span: Option<Span> = None;
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name == "slot" && first_slot_span.is_none() {
                    first_slot_span = Some(el.span);
                }
            }
        });

        let slot_span = match first_slot_span {
            Some(span) => span,
            None => return,
        };

        // Check both instance and module scripts for a `$$Slots` definition.
        let has_in_instance = ctx.ast.instance.as_ref()
            .map(|s| s.content.contains("$$Slots"))
            .unwrap_or(false);
        let has_in_module = ctx.ast.module.as_ref()
            .map(|m| m.content.contains("$$Slots"))
            .unwrap_or(false);

        if !has_in_instance && !has_in_module {
            ctx.diagnostic(
                "The component must define the $$Slots interface.",
                slot_span,
            );
        }
    }
}
