//! `svelte/prefer-writable-derived` — prefer `$derived` with a setter over manual patterns.
//! ⭐ Recommended 💡
//!
//! This rule requires semantic analysis to detect patterns where a `$derived` value
//! is manually synced back via effects. Currently a placeholder.

use crate::linter::{LintContext, Rule};

pub struct PreferWritableDerived;

impl Rule for PreferWritableDerived {
    fn name(&self) -> &'static str {
        "svelte/prefer-writable-derived"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // TODO: Requires semantic analysis to detect patterns where a $state + $effect
        // combination could be replaced with a writable $derived.
    }
}
