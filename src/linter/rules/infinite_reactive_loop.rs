//! `svelte/infinite-reactive-loop` — detect reactive statements that may cause infinite loops.
//! ⭐ Recommended
//!
//! This rule requires semantic analysis to properly detect reactive dependency cycles.
//! Currently a placeholder.

use crate::linter::{LintContext, Rule};

pub struct InfiniteReactiveLoop;

impl Rule for InfiniteReactiveLoop {
    fn name(&self) -> &'static str {
        "svelte/infinite-reactive-loop"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // TODO: Requires semantic analysis to trace reactive dependency graphs
        // and detect cycles that would cause infinite loops.
    }
}
