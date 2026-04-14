//! `svelte/no-extra-reactive-curlies` — disallow unnecessary curly braces in reactive statements.
//! 💡
//!
//! Detects `$: { single_statement; }` patterns where the braces are unnecessary.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::Statement;
use oxc::span::Span;

pub struct NoExtraReactiveCurlies;

impl Rule for NoExtraReactiveCurlies {
    fn name(&self) -> &'static str {
        "svelte/no-extra-reactive-curlies"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        for stmt in &semantic.nodes().program().body {
            let Statement::LabeledStatement(ls) = stmt else { continue };
            if ls.label.name != "$" {
                continue;
            }
            let Statement::BlockStatement(b) = &ls.body else { continue };
            // Flag only when the block contains a single statement — the braces
            // are unnecessary wrapper in that case.
            if b.body.len() != 1 {
                continue;
            }
            let s = content_offset + b.span.start;
            let e = content_offset + b.span.start + 1; // just the `{`
            ctx.diagnostic(
                "Do not wrap reactive statements in curly braces unless necessary.",
                Span::new(s, e),
            );
        }
    }
}
