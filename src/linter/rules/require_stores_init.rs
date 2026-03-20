//! `svelte/require-stores-init` — require store variables to be initialized.
//!
//! This rule requires semantic analysis to detect store declarations that lack
//! an initial value. Currently a placeholder.

use crate::linter::{LintContext, Rule};

pub struct RequireStoresInit;

impl Rule for RequireStoresInit {
    fn name(&self) -> &'static str {
        "svelte/require-stores-init"
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // TODO: Requires semantic analysis to detect writable/readable store
        // declarations that do not provide an initial value.
    }
}
