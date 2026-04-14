//! `svelte/require-store-callbacks-use-set-param` — require that store callbacks
//! use the `set` parameter provided by the callback.
//! 💡 Has suggestion

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Argument, BindingPattern, Expression, FormalParameters};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct RequireStoreCallbacksUseSetParam;

impl Rule for RequireStoreCallbacksUseSetParam {
    fn name(&self) -> &'static str {
        "svelte/require-store-callbacks-use-set-param"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        for node in semantic.nodes().iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            let Expression::Identifier(callee) = &ce.callee else { continue };
            if !matches!(callee.name.as_str(), "readable" | "writable") {
                continue;
            }
            // The callback is the 2nd argument.
            let Some(arg) = ce.arguments.get(1) else { continue };
            let (params, _is_arrow) = match arg {
                Argument::ArrowFunctionExpression(a) => (&a.params, true),
                Argument::FunctionExpression(f) => (&f.params, false),
                _ => continue,
            };
            if !has_set_param(params) {
                let s = content_offset + callee.span.start;
                let e = content_offset + callee.span.end + 1; // include `(`
                ctx.diagnostic(
                    "Store callbacks must use `set` param.",
                    Span::new(s, e),
                );
            }
        }
    }
}

fn has_set_param(params: &FormalParameters<'_>) -> bool {
    for p in &params.items {
        if let Some(name) = binding_name(&p.pattern) {
            if name == "set" {
                return true;
            }
        }
    }
    false
}

fn binding_name<'a>(pat: &'a BindingPattern<'a>) -> Option<&'a str> {
    match pat {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}
