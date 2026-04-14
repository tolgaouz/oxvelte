//! `svelte/no-inspect` — disallow use of `$inspect`.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct NoInspect;

impl Rule for NoInspect {
    fn name(&self) -> &'static str {
        "svelte/no-inspect"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for (sem, offset) in [
            (ctx.instance_semantic, ctx.instance_content_offset),
            (ctx.module_semantic, ctx.module_content_offset),
        ]
        .into_iter()
        .filter_map(|(s, o)| s.map(|s| (s, o)))
        {
            for node in sem.nodes().iter() {
                let AstKind::IdentifierReference(id) = node.kind() else { continue };
                if id.name != "$inspect" {
                    continue;
                }
                let s = offset + id.span.start;
                let e = offset + id.span.end;
                ctx.diagnostic(
                    "Do not use $inspect directive",
                    Span::new(s, e),
                );
            }
        }
    }
}
