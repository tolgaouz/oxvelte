//! `svelte/valid-style-parse` — report style parsing errors in `<style>` blocks.

use crate::linter::{LintContext, Rule};

pub struct ValidStyleParse;

impl Rule for ValidStyleParse {
    fn name(&self) -> &'static str {
        "svelte/valid-style-parse"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Placeholder: requires a CSS parser to validate the style block content.
        // A full implementation would parse `ctx.ast.css` and report syntax errors.
        let _ = ctx;
    }
}
