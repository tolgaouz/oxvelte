//! `svelte/prefer-writable-derived` — prefer `$derived` with a setter over `$state` + `$effect`.
//! ⭐ Recommended 💡

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{
    Argument, AssignmentTarget, Expression, Statement,
};
use oxc::ast::AstKind;
use oxc::span::Span;
use oxc::syntax::operator::AssignmentOperator;

pub struct PreferWritableDerived;

impl Rule for PreferWritableDerived {
    fn name(&self) -> &'static str {
        "svelte/prefer-writable-derived"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = ctx.ast.instance.as_ref() else { return };
        if !script.content.contains("$effect") || !script.content.contains("$state") {
            return;
        }
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;
        let scoping = semantic.scoping();
        let nodes = semantic.nodes();

        for node in nodes.iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            if !is_effect_or_effect_pre(&ce.callee) { continue; }
            if ce.arguments.len() != 1 { continue; }

            let Some(body_statements) = fn_arg_block_body(&ce.arguments[0]) else { continue };
            if body_statements.len() != 1 { continue; }
            let Statement::ExpressionStatement(es) = &body_statements[0] else { continue };
            let Expression::AssignmentExpression(ae) = &es.expression else { continue };
            if ae.operator != AssignmentOperator::Assign { continue; }
            let AssignmentTarget::AssignmentTargetIdentifier(id) = &ae.left else { continue };

            let Some(sid) = scoping.get_reference(id.reference_id()).symbol_id() else { continue };
            let decl_node_id = scoping.symbol_declaration(sid);
            let declarator = std::iter::once(decl_node_id)
                .chain(nodes.ancestor_ids(decl_node_id))
                .find_map(|nid| match nodes.kind(nid) {
                    AstKind::VariableDeclarator(d) => Some(d),
                    _ => None,
                });
            let Some(decl) = declarator else { continue };
            let Some(Expression::CallExpression(init_ce)) = &decl.init else { continue };
            let Expression::Identifier(init_id) = &init_ce.callee else { continue };
            if init_id.name != "$state" { continue; }

            let s = content_offset + decl.span.start;
            let e = content_offset + decl.span.end;
            ctx.diagnostic(
                "Prefer using writable $derived instead of $state and $effect",
                Span::new(s, e),
            );
        }
    }
}

fn is_effect_or_effect_pre(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::Identifier(id) => id.name == "$effect",
        Expression::StaticMemberExpression(mem) => {
            matches!(&mem.object, Expression::Identifier(id) if id.name == "$effect")
                && mem.property.name == "pre"
        }
        _ => false,
    }
}

/// Extract the block body of a `() => { ... }` or `function () { ... }` argument.
/// Returns the statements slice only when the function has zero parameters and
/// a block body (not an expression body).
fn fn_arg_block_body<'a>(arg: &'a Argument<'a>) -> Option<&'a [Statement<'a>]> {
    match arg {
        Argument::ArrowFunctionExpression(a) => {
            if !a.params.items.is_empty() || a.params.rest.is_some() { return None; }
            if a.expression { return None; }
            Some(a.body.statements.as_slice())
        }
        Argument::FunctionExpression(f) => {
            if !f.params.items.is_empty() || f.params.rest.is_some() { return None; }
            f.body.as_ref().map(|b| b.statements.as_slice())
        }
        _ => None,
    }
}
