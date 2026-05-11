//! `svelte/no-add-event-listener` — disallow `addEventListener` in Svelte components.
//! 💡

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::Expression;
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct NoAddEventListener;

impl Rule for NoAddEventListener {
    fn name(&self) -> &'static str {
        "svelte/no-add-event-listener"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Vendor condition: this rule only runs for Svelte 5 projects. Keep
        // unknown versions enabled so standalone fixtures still exercise it.
        if !ctx.svelte_version.is_unknown() && !ctx.svelte_version.includes_major(5) {
            return;
        }
        if let Some(semantic) = ctx.instance_semantic {
            check_semantic(ctx, semantic, ctx.instance_content_offset);
        }
        if let Some(semantic) = ctx.module_semantic {
            check_semantic(ctx, semantic, ctx.module_content_offset);
        }
    }
}

fn check_semantic(
    ctx: &mut LintContext<'_>,
    semantic: &oxc::semantic::Semantic<'_>,
    content_offset: u32,
) {
    for node in semantic.nodes().iter() {
        let AstKind::CallExpression(ce) = node.kind() else {
            continue;
        };
        let callee_span = match &ce.callee {
            // Bare call: `addEventListener('msg', handler)`
            Expression::Identifier(id) if id.name == "addEventListener" => id.span,
            // Member call: `window.addEventListener(...)`, `foo.bar.addEventListener(...)`
            Expression::StaticMemberExpression(mem) if mem.property.name == "addEventListener" => {
                mem.property.span
            }
            _ => continue,
        };
        let abs_start = content_offset + callee_span.start;
        let abs_end = content_offset + callee_span.end;
        ctx.diagnostic(
            "Do not use `addEventListener`. Use the `on` function from `svelte/events` instead.",
            Span::new(abs_start, abs_end),
        );
    }
}
