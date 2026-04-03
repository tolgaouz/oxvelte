//! `svelte/experimental-require-slot-types` — require slot types to be defined
//! for components that expose slots.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct ExperimentalRequireSlotTypes;

fn is_ts(lang: Option<&str>) -> bool {
    lang.map_or(false, |l| l.eq_ignore_ascii_case("ts") || l.eq_ignore_ascii_case("typescript"))
}

impl Rule for ExperimentalRequireSlotTypes {
    fn name(&self) -> &'static str { "svelte/experimental-require-slot-types" }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let scripts = [&ctx.ast.instance, &ctx.ast.module];
        if !scripts.iter().any(|s| s.as_ref().map_or(false, |s| is_ts(s.lang.as_deref()))) { return; }

        let mut slot_span: Option<Span> = None;
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name == "slot" && slot_span.is_none() { slot_span = Some(el.span); }
            }
        });
        let Some(span) = slot_span else { return };

        if !scripts.iter().any(|s| s.as_ref().map_or(false, |s| s.content.contains("$$Slots"))) {
            ctx.diagnostic("The component must define the $$Slots interface.", span);
        }
    }
}
