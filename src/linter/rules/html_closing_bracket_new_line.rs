//! `svelte/html-closing-bracket-new-line` — require or disallow a newline before
//! the closing bracket of elements.
//! 🔧 Fixable

use crate::linter::{LintContext, Rule};

pub struct HtmlClosingBracketNewLine;

impl Rule for HtmlClosingBracketNewLine {
    fn name(&self) -> &'static str {
        "svelte/html-closing-bracket-new-line"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Placeholder: requires precise source-position mapping of closing brackets
        // to determine whether they are on their own line.
        let _ = ctx;
    }
}
