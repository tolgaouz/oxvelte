//! `svelte/no-store-async` — disallow async functions in store callbacks.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Argument, Expression};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct NoStoreAsync;

impl Rule for NoStoreAsync {
    fn name(&self) -> &'static str {
        "svelte/no-store-async"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn applies_to_scripts(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        for node in semantic.nodes().iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            let Expression::Identifier(callee) = &ce.callee else { continue };
            let factory_name = callee.name.as_str();
            if !matches!(factory_name, "readable" | "writable" | "derived") {
                continue;
            }
            // Check the 2nd argument (callback). For readable/writable: (value, setter_fn).
            // For derived: (stores, fn, initial?).
            let Some(Argument::ArrowFunctionExpression(arr)) = ce.arguments.get(1).map(|a| &*a) else {
                // Also check function expressions
                if let Some(Argument::FunctionExpression(f)) = ce.arguments.get(1).map(|a| &*a) {
                    if f.r#async {
                        let s = content_offset + callee.span.start;
                        let e = content_offset + callee.span.end + 1; // include "("
                        ctx.diagnostic(
                            "Do not pass async functions to svelte stores.",
                            Span::new(s, e),
                        );
                    }
                }
                continue;
            };
            if arr.r#async {
                let s = content_offset + callee.span.start;
                let e = content_offset + callee.span.end + 1; // include "("
                ctx.diagnostic(
                    "Do not pass async functions to svelte stores.",
                    Span::new(s, e),
                );
            }
        }
    }
}
