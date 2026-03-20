//! `svelte/max-lines-per-block` — enforce a maximum number of lines in script/style blocks.

use crate::linter::{LintContext, Rule};

/// Default maximum number of lines allowed per block.
const DEFAULT_MAX_LINES: usize = 200;

pub struct MaxLinesPerBlock;

impl Rule for MaxLinesPerBlock {
    fn name(&self) -> &'static str {
        "svelte/max-lines-per-block"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Check instance script block
        if let Some(script) = &ctx.ast.instance {
            let line_count = script.content.matches('\n').count() + 1;
            if line_count > DEFAULT_MAX_LINES {
                ctx.diagnostic(
                    format!(
                        "Script block has {line_count} lines, which exceeds the maximum of {DEFAULT_MAX_LINES}."
                    ),
                    script.span,
                );
            }
        }

        // Check module script block
        if let Some(module) = &ctx.ast.module {
            let line_count = module.content.matches('\n').count() + 1;
            if line_count > DEFAULT_MAX_LINES {
                ctx.diagnostic(
                    format!(
                        "Module script block has {line_count} lines, which exceeds the maximum of {DEFAULT_MAX_LINES}."
                    ),
                    module.span,
                );
            }
        }

        // Check style block
        if let Some(style) = &ctx.ast.css {
            let line_count = style.content.matches('\n').count() + 1;
            if line_count > DEFAULT_MAX_LINES {
                ctx.diagnostic(
                    format!(
                        "Style block has {line_count} lines, which exceeds the maximum of {DEFAULT_MAX_LINES}."
                    ),
                    style.span,
                );
            }
        }
    }
}
