//! `svelte/no-store-async` — disallow async functions in store callbacks.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

pub struct NoStoreAsync;

impl Rule for NoStoreAsync {
    fn name(&self) -> &'static str {
        "svelte/no-store-async"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // Requires semantic analysis to detect store subscribe callbacks
        // Placeholder
    }
}
