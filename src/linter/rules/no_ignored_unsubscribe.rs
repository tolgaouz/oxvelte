//! `svelte/no-ignored-unsubscribe` — disallow ignoring store subscribe return value.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::Expression;
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct NoIgnoredUnsubscribe;

impl Rule for NoIgnoredUnsubscribe {
    fn name(&self) -> &'static str {
        "svelte/no-ignored-unsubscribe"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for (semantic, content_offset) in [
            ctx.instance_semantic
                .map(|s| (s, ctx.instance_content_offset)),
            ctx.module_semantic.map(|s| (s, ctx.module_content_offset)),
        ]
        .into_iter()
        .flatten()
        {
            let nodes = semantic.nodes();

            for node in nodes.iter() {
                let AstKind::CallExpression(ce) = node.kind() else {
                    continue;
                };
                let Expression::StaticMemberExpression(mem) = &ce.callee else {
                    continue;
                };
                if mem.property.name != "subscribe" {
                    continue;
                }
                // Report only when the call's value is ignored — i.e. its parent is
                // an `ExpressionStatement` directly. Assignments, declarations,
                // returns, or being passed as arguments all keep the unsubscribe
                // function reachable.
                let parent_kind = nodes.parent_kind(node.id());
                if !matches!(parent_kind, AstKind::ExpressionStatement(_)) {
                    continue;
                }
                let s = content_offset + ce.span.start;
                let e = content_offset + ce.span.end;
                ctx.diagnostic(
                    "Ignoring returned value of the subscribe method is forbidden.",
                    Span::new(s, e),
                );
            }
        }
    }
}
