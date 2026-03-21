//! `svelte/no-inner-declarations` — disallow variable or function declarations in nested blocks.
//! ⭐ Recommended (Extension Rule)

use crate::linter::{LintContext, Rule};

pub struct NoInnerDeclarations;

impl Rule for NoInnerDeclarations {
    fn name(&self) -> &'static str {
        "svelte/no-inner-declarations"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // This extension rule handles Svelte template-level {#if} blocks.
        // Requires JS scope analysis + template block awareness.
        // Placeholder — needs deep AST integration.
    }
}
