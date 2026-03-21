//! `svelte/no-navigation-without-base` — require navigation functions to use base path.

use crate::linter::{LintContext, Rule};

pub struct NoNavigationWithoutBase;

impl Rule for NoNavigationWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Placeholder: needs to check that navigation functions (pushState, replaceState, etc.)
        // use base from $app/paths
        let _ = ctx;
    }
}
