//! `svelte/valid-style-parse` — report style parsing errors in `<style>` blocks.

use crate::linter::{LintContext, Rule};

pub struct ValidStyleParse;

impl Rule for ValidStyleParse {
    fn name(&self) -> &'static str {
        "svelte/valid-style-parse"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Placeholder: the CSS parser needs strict error reporting
        // to detect malformed CSS. Currently it's too tolerant.
        let _ = ctx;
    }
}
