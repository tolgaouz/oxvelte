//! `svelte/no-not-function-handler` — disallow non-function event handlers.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, DirectiveKind, TemplateNode};
use oxc::allocator::Allocator;
use oxc::ast::ast::{Expression, VariableDeclarationKind};
use oxc::ast::AstKind;
use oxc::parser::Parser;
use oxc::semantic::Semantic;
use oxc::span::{Ident, SourceType, Span};

pub struct NoNotFunctionHandler;

impl Rule for NoNotFunctionHandler {
    fn name(&self) -> &'static str {
        "svelte/no-not-function-handler"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            for attr in &el.attributes {
                let handler_span = match attr {
                    Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } => *span,
                    Attribute::NormalAttribute { name, span, .. }
                        if name.starts_with("on")
                            && name.len() > 2
                            && name.as_bytes()[2].is_ascii_lowercase() =>
                    {
                        *span
                    }
                    _ => continue,
                };
                check_handler_value(ctx, handler_span);
            }
        });
    }
}

fn check_handler_value(ctx: &mut LintContext<'_>, span: Span) {
    let region = &ctx.source[span.start as usize..span.end as usize];
    // Handler value is expected as `={...}`. Extract the expression text.
    let Some(eq_pos) = region.find('=') else { return };
    let value = region[eq_pos + 1..].trim();
    if !(value.starts_with('{') && value.ends_with('}')) {
        return;
    }
    let expr_text = value[1..value.len() - 1].trim();
    if expr_text.is_empty() {
        return;
    }

    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, expr_text, SourceType::mjs()).parse_expression();
    let Ok(expr) = parsed else { return };

    let expr_span_start = span.start + region.find('{').map(|p| p + 1).unwrap_or(0) as u32;
    let ws_prefix_len = region[region.find('{').map(|p| p + 1).unwrap_or(0)..]
        .len()
        .saturating_sub(region[region.find('{').map(|p| p + 1).unwrap_or(0)..].trim_start().len());
    let expr_span_start = expr_span_start + ws_prefix_len as u32;
    let expr_span = Span::new(expr_span_start, expr_span_start + expr_text.len() as u32);

    if let Some(phrase) = non_function_phrase(&expr) {
        ctx.diagnostic(
            format!("Unexpected {} in event handler.", phrase),
            expr_span,
        );
    } else if let Expression::Identifier(id) = &expr {
        // Follow variable reference to its initializer in the instance script.
        if let Some(sem) = ctx.instance_semantic {
            if let Some(phrase) = identifier_non_function_phrase(id.name.as_str(), sem) {
                ctx.diagnostic(
                    format!("Unexpected {} in event handler.", phrase),
                    expr_span,
                );
            }
        }
    }
}

/// If `expr` is a plainly non-function literal, return a descriptive phrase.
fn non_function_phrase(expr: &Expression<'_>) -> Option<&'static str> {
    match expr {
        Expression::ArrayExpression(_) => Some("array"),
        Expression::ObjectExpression(_) => Some("object"),
        Expression::StringLiteral(_) => Some("string value"),
        Expression::TemplateLiteral(_) => Some("string value"),
        Expression::BooleanLiteral(_) => Some("boolean value"),
        Expression::NumericLiteral(_) => Some("number value"),
        Expression::BigIntLiteral(_) => Some("bigint value"),
        Expression::RegExpLiteral(_) => Some("regex value"),
        Expression::ClassExpression(_) => Some("class"),
        Expression::NewExpression(_) => Some("new expression"),
        _ => None,
    }
}

/// Given an identifier name, look up its declaration in the instance-script
/// root scope. If it's initialized with a non-function literal, return the
/// descriptive phrase.
fn identifier_non_function_phrase<'a>(
    name: &str,
    sem: &'a Semantic<'a>,
) -> Option<&'static str> {
    let scoping = sem.scoping();
    let sid = scoping.find_binding(scoping.root_scope_id(), Ident::new_const(name))?;
    // Skip function declarations (these ARE functions).
    let flags = scoping.symbol_flags(sid);
    if flags.intersects(oxc::semantic::SymbolFlags::Function) {
        return None;
    }
    // Find the VariableDeclarator for this symbol.
    let decl_node_id = scoping.symbol_declaration(sid);
    let vd = std::iter::once(decl_node_id)
        .chain(sem.nodes().ancestor_ids(decl_node_id))
        .find_map(|aid| match sem.nodes().kind(aid) {
            AstKind::VariableDeclarator(vd) => Some(vd),
            _ => None,
        })?;
    // Only `const` initializers can be confidently classified (a `let`/`var`
    // could be reassigned to a function later).
    let decl_kind = std::iter::once(decl_node_id)
        .chain(sem.nodes().ancestor_ids(decl_node_id))
        .find_map(|aid| match sem.nodes().kind(aid) {
            AstKind::VariableDeclaration(d) => Some(d.kind),
            _ => None,
        })?;
    if decl_kind != VariableDeclarationKind::Const {
        return None;
    }
    let init = vd.init.as_ref()?;
    non_function_phrase(init)
}
