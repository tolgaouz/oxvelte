//! `svelte/block-lang` — enforce or disallow specific `lang` attributes on script/style blocks.
//! 💡

use crate::linter::{LintContext, Rule};

pub struct BlockLang;

impl Rule for BlockLang {
    fn name(&self) -> &'static str {
        "svelte/block-lang"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Check script block lang attribute
        if let Some(script) = &ctx.ast.instance {
            if script.lang.is_none() {
                ctx.diagnostic(
                    "Script block should specify a `lang` attribute (e.g. `lang=\"ts\"`).",
                    script.span,
                );
            }
        }
        if let Some(module) = &ctx.ast.module {
            if module.lang.is_none() {
                ctx.diagnostic(
                    "Module script block should specify a `lang` attribute (e.g. `lang=\"ts\"`).",
                    module.span,
                );
            }
        }

        // Check style block lang attribute
        if let Some(style) = &ctx.ast.css {
            if style.lang.is_none() {
                ctx.diagnostic(
                    "Style block should specify a `lang` attribute (e.g. `lang=\"scss\"`).",
                    style.span,
                );
            }
        }
    }
}
