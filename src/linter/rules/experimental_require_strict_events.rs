//! `svelte/experimental-require-strict-events` — require strict event typing.

use crate::linter::{LintContext, Rule};

fn is_ts_lang(lang: Option<&str>) -> bool {
    match lang {
        Some(l) => l.eq_ignore_ascii_case("ts") || l.eq_ignore_ascii_case("typescript"),
        None => false,
    }
}

pub struct ExperimentalRequireStrictEvents;

impl Rule for ExperimentalRequireStrictEvents {
    fn name(&self) -> &'static str {
        "svelte/experimental-require-strict-events"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Only applies to TypeScript components
        let instance_is_ts = ctx.ast.instance.as_ref()
            .map(|s| is_ts_lang(s.lang.as_deref()))
            .unwrap_or(false);
        let module_is_ts = ctx.ast.module.as_ref()
            .map(|s| is_ts_lang(s.lang.as_deref()))
            .unwrap_or(false);
        if !instance_is_ts && !module_is_ts { return; }

        // Check for strictEvents attribute on the script tag
        let script = ctx.ast.instance.as_ref().unwrap();
        let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
        let tag_attrs = tag_text.split('>').next().unwrap_or("");
        if tag_attrs.contains("strictEvents") {
            return;
        }

        // Check if the component defines strict events via $$Events type
        let has_events_type = script.content.contains("$$Events")
            || ctx.ast.module.as_ref()
                .map(|s| s.content.contains("$$Events"))
                .unwrap_or(false);

        if !has_events_type {
            let span = ctx.ast.instance.as_ref()
                .map(|s| s.span)
                .unwrap_or(ctx.ast.html.span);
            ctx.diagnostic(
                "The component must have the strictEvents attribute on its <script> tag or it must define the $$Events interface.",
                span,
            );
        }
    }
}
