//! `svelte/experimental-require-strict-events` — require strict event typing.

use crate::linter::{LintContext, Rule};

pub struct ExperimentalRequireStrictEvents;

impl Rule for ExperimentalRequireStrictEvents {
    fn name(&self) -> &'static str { "svelte/experimental-require-strict-events" }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let is_ts = |s: &crate::ast::Script| matches!(s.lang.as_deref(), Some("ts" | "typescript" | "TS" | "Typescript" | "TypeScript"));
        let scripts = [&ctx.ast.instance, &ctx.ast.module];
        if !scripts.iter().any(|s| s.as_ref().map_or(false, |s| is_ts(s))) { return; }
        let Some(script) = ctx.ast.instance.as_ref() else { return };
        let tag = &ctx.source[script.span.start as usize..script.span.end as usize];
        if tag.split('>').next().unwrap_or("").contains("strictEvents") { return; }
        if script.content.contains("$$Events") || ctx.ast.module.as_ref().map_or(false, |m| m.content.contains("$$Events")) { return; }
        ctx.diagnostic("The component must have the strictEvents attribute on its <script> tag or it must define the $$Events interface.", script.span);
    }
}
