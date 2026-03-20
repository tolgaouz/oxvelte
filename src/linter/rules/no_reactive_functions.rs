//! `svelte/no-reactive-functions` — disallow reactive declarations with function calls that don't access reactive values.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};

pub struct NoReactiveFunctions;

impl Rule for NoReactiveFunctions {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-functions"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // This rule requires semantic analysis to determine if a function accesses reactive values
        // Placeholder — full implementation needs scope analysis
    }
}
