//! `svelte/no-not-function-handler` — disallow non-function event handlers.
//! ⭐ Recommended

use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};
use crate::linter::{walk_template_nodes, LintContext, Rule};
use oxc::ast::ast::{Expression, VariableDeclarationKind};
use oxc::ast::AstKind;
use oxc::semantic::Semantic;
use oxc::span::Span;

pub struct NoNotFunctionHandler;

impl Rule for NoNotFunctionHandler {
    fn name(&self) -> &'static str {
        "svelte/no-not-function-handler"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let mut findings: Vec<(String, Span)> = Vec::new();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };
            for (idx, attr) in el.attributes.iter().enumerate() {
                match attr {
                    // `on:event={…}` directive — typed AST is in attribute_meta[idx].
                    Attribute::Directive {
                        kind: DirectiveKind::EventHandler,
                        ..
                    } => {
                        let Some(expr) = el.attribute_expression_ast(idx) else { continue };
                        check_handler(expr, ctx, &mut findings, attr_value_span(attr));
                    }
                    // `onclick={…}` Svelte-5 / HTML on-attribute. Restricted to
                    // names matching the curated `is_event_name` test.
                    Attribute::NormalAttribute { name, value, .. } if is_event_name(name) => {
                        match value {
                            AttributeValue::Expression(_) => {
                                let Some(expr) = el.attribute_expression_ast(idx) else { continue };
                                check_handler(expr, ctx, &mut findings, attr_value_span(attr));
                            }
                            AttributeValue::Concat(parts) => {
                                for (part_idx, part) in parts.iter().enumerate() {
                                    if !matches!(part, AttributeValuePart::Expression(_)) {
                                        continue;
                                    }
                                    let Some(expr) =
                                        el.attribute_part_expression_ast(idx, part_idx)
                                    else {
                                        continue;
                                    };
                                    let span = el
                                        .attribute_meta
                                        .get(idx)
                                        .and_then(|m| m.parts.get(part_idx))
                                        .and_then(|p| p.expression_span)
                                        .unwrap_or_else(|| attr_value_span(attr));
                                    check_handler(expr, ctx, &mut findings, span);
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        });
        for (msg, span) in findings {
            ctx.diagnostic(msg, span);
        }
    }
}

/// True for any HTML event-handler attribute name. Mirrors the shape of
/// vendor's `EVENT_NAMES` table without enumerating the full ~370-entry
/// list: matches `on[lowercase][a-z0-9]*`. `oncology` (a non-event
/// camelCase noun) doesn't survive because the `[2..]` portion has to be
/// purely alphanumeric *and* start with a lowercase ASCII letter — covers
/// the real event surface tightly enough for a linter.
fn is_event_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 3 || &bytes[..2] != b"on" {
        return false;
    }
    if !bytes[2].is_ascii_lowercase() {
        return false;
    }
    bytes[2..].iter().all(|b| b.is_ascii_alphanumeric())
}

fn attr_value_span(attr: &Attribute) -> Span {
    match attr {
        Attribute::NormalAttribute { span, .. } | Attribute::Directive { span, .. } => *span,
        Attribute::Spread { span } => *span,
    }
}

fn check_handler<'a>(
    expr: &'a Expression<'a>,
    ctx: &LintContext<'a>,
    findings: &mut Vec<(String, Span)>,
    span: Span,
) {
    // Vendor's `findRootExpression`: follow `const` aliases all the way down.
    let resolved = if let Expression::Identifier(id) = expr {
        ctx.instance_semantic
            .and_then(|sem| resolve_const_init(id.name.as_str(), sem))
            .unwrap_or(expr)
    } else {
        expr
    };
    if let Some(phrase) = non_function_phrase(resolved) {
        findings.push((format!("Unexpected {} in event handler.", phrase), span));
    }
}

/// Vendor's PHRASES table. `null` literal returns `None` (vendor's
/// `node.value == null` short-circuit), so `onclick={null}` is silently
/// allowed. `NewExpression` is *not* in the table — `new Decorator()` may
/// well return a function.
fn non_function_phrase(expr: &Expression<'_>) -> Option<&'static str> {
    match expr {
        Expression::ArrayExpression(_) => Some("array"),
        Expression::ObjectExpression(_) => Some("object"),
        Expression::ClassExpression(_) => Some("class"),
        Expression::StringLiteral(_) => Some("string value"),
        Expression::TemplateLiteral(_) => Some("string value"),
        Expression::BooleanLiteral(_) => Some("boolean value"),
        Expression::NumericLiteral(_) => Some("number value"),
        Expression::BigIntLiteral(_) => Some("bigint value"),
        Expression::RegExpLiteral(_) => Some("regex value"),
        _ => None,
    }
}

/// Look up `name` in the instance-script root scope. If it's bound by
/// `const X = init`, return `init` (recursively, so chains of `const`
/// renames are followed). Otherwise `None`.
fn resolve_const_init<'a>(name: &str, sem: &'a Semantic<'a>) -> Option<&'a Expression<'a>> {
    let scoping = sem.scoping();
    let sid = scoping.find_binding(scoping.root_scope_id(), name.into())?;
    if scoping
        .symbol_flags(sid)
        .intersects(oxc::semantic::SymbolFlags::Function)
    {
        return None;
    }
    let decl_node_id = scoping.symbol_declaration(sid);
    let nodes = sem.nodes();
    let vd = std::iter::once(decl_node_id)
        .chain(nodes.ancestor_ids(decl_node_id))
        .find_map(|aid| match nodes.kind(aid) {
            AstKind::VariableDeclarator(vd) => Some(vd),
            _ => None,
        })?;
    let decl_kind = std::iter::once(decl_node_id)
        .chain(nodes.ancestor_ids(decl_node_id))
        .find_map(|aid| match nodes.kind(aid) {
            AstKind::VariableDeclaration(d) => Some(d.kind),
            _ => None,
        })?;
    if decl_kind != VariableDeclarationKind::Const {
        return None;
    }
    let init = vd.init.as_ref()?;
    // Recursively follow chained const aliases.
    if let Expression::Identifier(inner) = init {
        return resolve_const_init(inner.name.as_str(), sem).or(Some(init));
    }
    Some(init)
}
