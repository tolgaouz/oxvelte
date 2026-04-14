//! `svelte/derived-has-same-inputs-outputs` — require `$derived` stores to use the
//! same names for inputs and outputs.
//! 💡 Has suggestion

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Argument, BindingPattern, Expression};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct DerivedHasSameInputsOutputs;

impl Rule for DerivedHasSameInputsOutputs {
    fn name(&self) -> &'static str {
        "svelte/derived-has-same-inputs-outputs"
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
            if callee.name != "derived" {
                continue;
            }
            // derived(store, ({$store}) => ...)
            let store_name = ce.arguments.first().and_then(|a| match a {
                Argument::Identifier(id) => Some(id.name.as_str()),
                _ => None,
            });
            let Some(store_name) = store_name else { continue };
            let param_name = ce.arguments.get(1).and_then(|a| match a {
                Argument::ArrowFunctionExpression(arr) => param_binding_name(&arr.params.items),
                Argument::FunctionExpression(f) => param_binding_name(&f.params.items),
                _ => None,
            });
            let Some(param_name) = param_name else { continue };
            let expected = format!("${}", store_name);
            // If the param matches the expected $-prefixed name OR the store's
            // own name, accept.
            if param_name == expected.as_str() || param_name == store_name {
                continue;
            }
            let s = content_offset + callee.span.start;
            let e = content_offset + callee.span.end + 1;
            ctx.diagnostic(
                format!("The argument name should be '{}'.", expected),
                Span::new(s, e),
            );
        }
    }
}

fn param_binding_name<'a>(
    params: &'a oxc::allocator::Vec<'a, oxc::ast::ast::FormalParameter<'a>>,
) -> Option<&'a str> {
    let p = params.first()?;
    match &p.pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}
