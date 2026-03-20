//! `svelte/no-unused-props` — disallow unused component props.
//! ⭐ Recommended
//!
//! This rule requires semantic analysis to track prop declarations and their usage
//! across template and script blocks. Currently a placeholder.

use crate::linter::{LintContext, Rule};

pub struct NoUnusedProps;

impl Rule for NoUnusedProps {
    fn name(&self) -> &'static str {
        "svelte/no-unused-props"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // TODO: Requires semantic analysis to:
        // 1. Collect all `export let` / `$props()` declarations
        // 2. Track usage in both script and template
        // 3. Report props that are declared but never referenced
    }
}
