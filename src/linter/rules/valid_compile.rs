//! `svelte/valid-compile` — report Svelte compilation errors / warnings.
//! Requires svelte-ignore comment support for full implementation.

use crate::linter::{LintContext, Rule};

pub struct ValidCompile;

impl Rule for ValidCompile {
    fn name(&self) -> &'static str {
        "svelte/valid-compile"
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // Requires integration with the Svelte compiler and svelte-ignore
        // comment processing to surface compile-time warnings as diagnostics.
    }
}
