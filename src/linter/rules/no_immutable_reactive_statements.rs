//! `svelte/no-immutable-reactive-statements` — disallow reactive statements that don't reference reactive values.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

pub struct NoImmutableReactiveStatements;

impl Rule for NoImmutableReactiveStatements {
    fn name(&self) -> &'static str {
        "svelte/no-immutable-reactive-statements"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // Requires deep mutation/scope analysis to determine if referenced
        // variables are truly immutable (const binding != immutable contents).
        // Placeholder — needs semantic analysis with type tracking.
    }
}
