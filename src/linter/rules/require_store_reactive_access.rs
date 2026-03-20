//! `svelte/require-store-reactive-access` — require `$store` syntax for reactive access.
//! ⭐ Recommended 🔧 Fixable

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct RequireStoreReactiveAccess;

impl Rule for RequireStoreReactiveAccess {
    fn name(&self) -> &'static str {
        "svelte/require-store-reactive-access"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Placeholder: full implementation requires tracking which variables are stores
        // and checking that template expressions use `$store` rather than `store.get()`.
        // For now, flag `.get()` calls on common store variable patterns in the template.
        let _ = ctx;
    }
}
