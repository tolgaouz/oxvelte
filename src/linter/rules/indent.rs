//! `svelte/indent` — enforce consistent indentation.
//! 🔧 Fixable

use crate::linter::{LintContext, Rule};

pub struct Indent;

impl Rule for Indent {
    fn name(&self) -> &'static str {
        "svelte/indent"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Placeholder: full indent checking requires tracking block nesting depth
        // and comparing leading whitespace against expected indentation at each level.
        let _ = ctx;
    }
}
