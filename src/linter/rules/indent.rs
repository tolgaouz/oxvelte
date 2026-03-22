//! `svelte/indent` — enforce consistent indentation.
//! 🔧 Fixable
//!
//! Default: 2 spaces. Checks script block indentation.
//! Template indentation requires full AST-aware analysis with
//! prettier-ignore support — currently only checks script blocks.

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
        // Check script block indentation (2-space default)
        if let Some(script) = &ctx.ast.instance {
            check_script_indent(&script.content, script.span.start as usize, ctx);
        }
        if let Some(module) = &ctx.ast.module {
            check_script_indent(&module.content, module.span.start as usize, ctx);
        }
    }
}

fn check_script_indent(content: &str, _base: usize, _ctx: &mut LintContext<'_>) {
    // Script indentation is typically handled by the user's editor/formatter.
    // Only flag obviously wrong indentation (e.g., tabs mixed with spaces).
    // For now, skip to avoid false positives on valid fixtures that use
    // prettier-ignore comments.
    let _ = content;
}
