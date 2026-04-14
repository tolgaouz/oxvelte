//! `svelte/experimental-require-strict-events` — require strict event typing.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::Statement;

pub struct ExperimentalRequireStrictEvents;

impl Rule for ExperimentalRequireStrictEvents {
    fn name(&self) -> &'static str {
        "svelte/experimental-require-strict-events"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let is_ts = |s: &crate::ast::Script| {
            matches!(s.lang.as_deref(), Some("ts" | "typescript" | "TS" | "Typescript" | "TypeScript"))
        };
        let scripts = [&ctx.ast.instance, &ctx.ast.module];
        if !scripts.iter().any(|s| s.as_ref().map_or(false, |s| is_ts(s))) {
            return;
        }
        let Some(script) = ctx.ast.instance.as_ref() else { return };
        // `<script strictEvents>` opt-out — need to look at the open tag attributes.
        let tag = &ctx.source[script.span.start as usize..script.span.end as usize];
        if tag.split('>').next().unwrap_or("").contains("strictEvents") {
            return;
        }
        // AST check: a top-level `interface $$Events` or `type $$Events` in
        // either instance or module script.
        let has_events = [ctx.instance_semantic, ctx.module_semantic]
            .iter()
            .filter_map(|s| *s)
            .any(|sem| {
                sem.nodes().program().body.iter().any(|stmt| match stmt {
                    Statement::TSInterfaceDeclaration(i) => i.id.name == "$$Events",
                    Statement::TSTypeAliasDeclaration(t) => t.id.name == "$$Events",
                    _ => false,
                })
            });
        if !has_events {
            ctx.diagnostic(
                "The component must have the strictEvents attribute on its <script> tag or it must define the $$Events interface.",
                script.span,
            );
        }
    }
}
