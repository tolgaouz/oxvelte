//! `svelte/comment-directive` — support HTML eslint comment directives.
//!
//! Directive suppression is handled centrally in `linter::filter_suppressed`.
//! This rule exists so the vendor rule name can be enabled/disabled without
//! adding duplicate `svelte-ignore` diagnostics here.

use crate::linter::{LintContext, Rule};

pub struct CommentDirective;

impl Rule for CommentDirective {
    fn name(&self) -> &'static str {
        "svelte/comment-directive"
    }

    fn run<'a>(&self, _ctx: &mut LintContext<'a>) {
        // Vendor stores HTML eslint-disable state in shared parser services.
        // Oxvelte applies equivalent suppression centrally after all rules run.
    }
}
