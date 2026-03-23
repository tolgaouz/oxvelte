//! `svelte/experimental-require-strict-events` — require strict event typing.

use crate::linter::{LintContext, Rule};

pub struct ExperimentalRequireStrictEvents;

impl Rule for ExperimentalRequireStrictEvents {
    fn name(&self) -> &'static str {
        "svelte/experimental-require-strict-events"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Only applies to TypeScript components
        let is_ts = ctx.ast.instance.as_ref()
            .map(|s| s.lang.as_deref() == Some("ts"))
            .unwrap_or(false);
        if !is_ts { return; }

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

        // Check if createEventDispatcher is used with type parameters
        let has_typed_dispatcher = script.content.contains("createEventDispatcher<");

        if !has_events_type && !has_typed_dispatcher {
            let span = ctx.ast.instance.as_ref()
                .map(|s| s.span)
                .unwrap_or(ctx.ast.html.span);
            ctx.diagnostic(
                "Component should define strict event types using `$$Events` or typed `createEventDispatcher<Events>()`.",
                span,
            );
        }
    }
}
