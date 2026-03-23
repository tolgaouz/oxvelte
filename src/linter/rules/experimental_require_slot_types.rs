//! `svelte/experimental-require-slot-types` — require slot types to be defined
//! for components that expose slots.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct ExperimentalRequireSlotTypes;

impl Rule for ExperimentalRequireSlotTypes {
    fn name(&self) -> &'static str {
        "svelte/experimental-require-slot-types"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Only applies to TypeScript scripts
        let is_ts = ctx.ast.instance.as_ref().map(|s| s.lang.as_deref() == Some("ts")).unwrap_or(false);
        if !is_ts { return; }

        // Check if the component has <slot> elements.
        let mut has_slot = false;
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name == "slot" {
                    has_slot = true;
                }
            }
        });

        if !has_slot {
            return;
        }

        // Check that the module script exports a `$$Slots` type.
        let has_slot_types = ctx
            .ast
            .module
            .as_ref()
            .map(|m| m.content.contains("$$Slots"))
            .unwrap_or(false);

        if !has_slot_types {
            // Also check the instance script.
            let has_in_instance = ctx
                .ast
                .instance
                .as_ref()
                .map(|s| s.content.contains("$$Slots"))
                .unwrap_or(false);
            if !has_in_instance {
                ctx.diagnostic(
                    "Component uses `<slot>` but does not define `$$Slots` type.",
                    ctx.ast.html.span,
                );
            }
        }
    }
}
