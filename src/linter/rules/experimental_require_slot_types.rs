//! `svelte/experimental-require-slot-types` — require slot types to be defined
//! for components that expose slots.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::ast::ast::Statement;
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct ExperimentalRequireSlotTypes;

fn is_ts(lang: Option<&str>) -> bool {
    lang.map_or(false, |l| l.eq_ignore_ascii_case("ts") || l.eq_ignore_ascii_case("typescript"))
}

impl Rule for ExperimentalRequireSlotTypes {
    fn name(&self) -> &'static str {
        "svelte/experimental-require-slot-types"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let is_ts_file = [&ctx.ast.instance, &ctx.ast.module]
            .iter()
            .any(|s| s.as_ref().map_or(false, |s| is_ts(s.lang.as_deref())));
        if !is_ts_file {
            return;
        }

        let mut slot_span: Option<Span> = None;
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name == "slot" && slot_span.is_none() {
                    slot_span = Some(el.span);
                }
            }
        });
        let Some(span) = slot_span else { return };

        // Check for `$$Slots` declaration in either semantic (instance or module).
        let has_slots = [ctx.instance_semantic, ctx.module_semantic]
            .iter()
            .filter_map(|s| *s)
            .any(|sem| {
                sem.nodes().iter().any(|n| match n.kind() {
                    AstKind::TSInterfaceDeclaration(i) => i.id.name == "$$Slots",
                    AstKind::TSTypeAliasDeclaration(t) => t.id.name == "$$Slots",
                    _ => false,
                }) || sem.nodes().program().body.iter().any(|stmt| match stmt {
                    Statement::TSInterfaceDeclaration(i) => i.id.name == "$$Slots",
                    Statement::TSTypeAliasDeclaration(t) => t.id.name == "$$Slots",
                    _ => false,
                })
            });

        if !has_slots {
            ctx.diagnostic("The component must define the $$Slots interface.", span);
        }
    }
}
