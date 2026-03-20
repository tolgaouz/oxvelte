//! Serialize oxvelte AST to the Svelte compiler's legacy JSON format.
//!
//! This module converts our internal AST representation into `serde_json::Value`
//! matching the expected output from the Svelte 4 compiler's parser, so we can
//! compare against the test fixtures in `fixtures/parser/legacy/`.

use serde_json::{json, Value};
use crate::ast::*;

/// Compute line/column location info from a byte offset in source text.
/// Line numbers are 1-based, columns are 0-based.
fn offset_to_loc(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn loc_json(source: &str, start: u32, end: u32) -> Value {
    let (sl, sc) = offset_to_loc(source, start as usize);
    let (el, ec) = offset_to_loc(source, end as usize);
    json!({
        "start": { "line": sl, "column": sc },
        "end": { "line": el, "column": ec }
    })
}

fn loc_json_with_char(source: &str, start: u32, end: u32) -> Value {
    let (sl, sc) = offset_to_loc(source, start as usize);
    let (el, ec) = offset_to_loc(source, end as usize);
    json!({
        "start": { "line": sl, "column": sc, "character": start },
        "end": { "line": el, "column": ec, "character": end }
    })
}

/// Parse a JS expression string with oxc and serialize to estree JSON.
fn expression_to_estree(source: &str, expr_str: &str, expr_start: u32) -> Value {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let alloc = Allocator::default();
    let source_type = SourceType::mjs();
    let result = Parser::new(&alloc, expr_str, source_type).parse_expression();

    match result {
        Ok(expr) => estree_expr(&expr, source, expr_start),
        Err(_) => {
            // Fallback: treat as raw identifier
            json!({
                "type": "Identifier",
                "start": expr_start,
                "end": expr_start + expr_str.len() as u32,
                "loc": loc_json(source, expr_start, expr_start + expr_str.len() as u32),
                "name": expr_str
            })
        }
    }
}

/// Convert an oxc Expression AST node to estree JSON.
fn estree_expr(expr: &oxc::ast::ast::Expression<'_>, source: &str, offset: u32) -> Value {
    use oxc::ast::ast::Expression;
    match expr {
        Expression::Identifier(ident) => {
            let start = offset + ident.span.start;
            let end = offset + ident.span.end;
            json!({
                "type": "Identifier",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "name": ident.name.as_str()
            })
        }
        Expression::StringLiteral(lit) => {
            let start = offset + lit.span.start;
            let end = offset + lit.span.end;
            json!({
                "type": "Literal",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "value": lit.value.as_str(),
                "raw": &source[start as usize..end as usize]
            })
        }
        Expression::NumericLiteral(lit) => {
            let start = offset + lit.span.start;
            let end = offset + lit.span.end;
            // Serialize integers as integers, not floats
            let value = if lit.value.fract() == 0.0 && lit.value.abs() < (i64::MAX as f64) {
                json!(lit.value as i64)
            } else {
                json!(lit.value)
            };
            json!({
                "type": "Literal",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "value": value,
                "raw": &source[start as usize..end as usize]
            })
        }
        Expression::BooleanLiteral(lit) => {
            let start = offset + lit.span.start;
            let end = offset + lit.span.end;
            json!({
                "type": "Literal",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "value": lit.value,
                "raw": if lit.value { "true" } else { "false" }
            })
        }
        Expression::NullLiteral(lit) => {
            let start = offset + lit.span.start;
            let end = offset + lit.span.end;
            json!({
                "type": "Literal",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "value": null,
                "raw": "null"
            })
        }
        Expression::CallExpression(call) => {
            let start = offset + call.span.start;
            let end = offset + call.span.end;
            let callee = estree_expr_from_callee(&call.callee, source, offset);
            let args: Vec<Value> = call.arguments.iter().map(|a| {
                match a {
                    oxc::ast::ast::Argument::SpreadElement(s) => {
                        let s_start = offset + s.span.start;
                        let s_end = offset + s.span.end;
                        json!({
                            "type": "SpreadElement",
                            "start": s_start,
                            "end": s_end,
                            "argument": estree_expr(&s.argument, source, offset)
                        })
                    }
                    _ => {
                        // Argument is an Expression
                        estree_expr(a.as_expression().unwrap(), source, offset)
                    }
                }
            }).collect();
            json!({
                "type": "CallExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "callee": callee,
                "arguments": args,
                "optional": false
            })
        }
        Expression::StaticMemberExpression(mem) => {
            let start = offset + mem.span.start;
            let end = offset + mem.span.end;
            let object = estree_expr(&mem.object, source, offset);
            let prop_start = offset + mem.property.span.start;
            let prop_end = offset + mem.property.span.end;
            json!({
                "type": "MemberExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "object": object,
                "property": {
                    "type": "Identifier",
                    "start": prop_start,
                    "end": prop_end,
                    "loc": loc_json(source, prop_start, prop_end),
                    "name": mem.property.name.as_str()
                },
                "computed": false,
                "optional": false
            })
        }
        Expression::ComputedMemberExpression(mem) => {
            let start = offset + mem.span.start;
            let end = offset + mem.span.end;
            let object = estree_expr(&mem.object, source, offset);
            let property = estree_expr(&mem.expression, source, offset);
            json!({
                "type": "MemberExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "object": object,
                "property": property,
                "computed": true,
                "optional": false
            })
        }
        Expression::BinaryExpression(bin) => {
            let start = offset + bin.span.start;
            let end = offset + bin.span.end;
            json!({
                "type": "BinaryExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "left": estree_expr(&bin.left, source, offset),
                "operator": bin.operator.as_str(),
                "right": estree_expr(&bin.right, source, offset)
            })
        }
        Expression::LogicalExpression(log) => {
            let start = offset + log.span.start;
            let end = offset + log.span.end;
            json!({
                "type": "LogicalExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "left": estree_expr(&log.left, source, offset),
                "operator": log.operator.as_str(),
                "right": estree_expr(&log.right, source, offset)
            })
        }
        Expression::UnaryExpression(un) => {
            let start = offset + un.span.start;
            let end = offset + un.span.end;
            json!({
                "type": "UnaryExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "operator": un.operator.as_str(),
                "prefix": true,
                "argument": estree_expr(&un.argument, source, offset)
            })
        }
        Expression::ConditionalExpression(cond) => {
            let start = offset + cond.span.start;
            let end = offset + cond.span.end;
            json!({
                "type": "ConditionalExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "test": estree_expr(&cond.test, source, offset),
                "consequent": estree_expr(&cond.consequent, source, offset),
                "alternate": estree_expr(&cond.alternate, source, offset)
            })
        }
        Expression::TemplateLiteral(tl) => {
            let start = offset + tl.span.start;
            let end = offset + tl.span.end;
            let quasis: Vec<Value> = tl.quasis.iter().map(|q| {
                let q_start = offset + q.span.start;
                let q_end = offset + q.span.end;
                json!({
                    "type": "TemplateElement",
                    "start": q_start,
                    "end": q_end,
                    "value": {
                        "raw": q.value.raw.as_str(),
                        "cooked": q.value.cooked.as_ref().map(|c| c.as_str())
                    },
                    "tail": q.tail
                })
            }).collect();
            let exprs: Vec<Value> = tl.expressions.iter().map(|e| {
                estree_expr(e, source, offset)
            }).collect();
            json!({
                "type": "TemplateLiteral",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "expressions": exprs,
                "quasis": quasis
            })
        }
        Expression::ArrayExpression(arr) => {
            let start = offset + arr.span.start;
            let end = offset + arr.span.end;
            let elements: Vec<Value> = arr.elements.iter().map(|el| {
                match el {
                    oxc::ast::ast::ArrayExpressionElement::SpreadElement(s) => {
                        let s_start = offset + s.span.start;
                        let s_end = offset + s.span.end;
                        json!({
                            "type": "SpreadElement",
                            "start": s_start,
                            "end": s_end,
                            "argument": estree_expr(&s.argument, source, offset)
                        })
                    }
                    oxc::ast::ast::ArrayExpressionElement::Elision(e) => {
                        Value::Null
                    }
                    _ => {
                        estree_expr(el.as_expression().unwrap(), source, offset)
                    }
                }
            }).collect();
            json!({
                "type": "ArrayExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "elements": elements
            })
        }
        Expression::ObjectExpression(obj) => {
            let start = offset + obj.span.start;
            let end = offset + obj.span.end;
            let properties: Vec<Value> = obj.properties.iter().map(|prop| {
                match prop {
                    oxc::ast::ast::ObjectPropertyKind::ObjectProperty(p) => {
                        let p_start = offset + p.span.start;
                        let p_end = offset + p.span.end;
                        let key = estree_property_key(&p.key, source, offset);
                        let value = estree_expr(&p.value, source, offset);
                        json!({
                            "type": "Property",
                            "start": p_start,
                            "end": p_end,
                            "loc": loc_json(source, p_start, p_end),
                            "method": p.method,
                            "shorthand": p.shorthand,
                            "computed": p.computed,
                            "key": key,
                            "value": value,
                            "kind": match p.kind {
                                oxc::ast::ast::PropertyKind::Init => "init",
                                oxc::ast::ast::PropertyKind::Get => "get",
                                oxc::ast::ast::PropertyKind::Set => "set",
                            }
                        })
                    }
                    oxc::ast::ast::ObjectPropertyKind::SpreadProperty(s) => {
                        let s_start = offset + s.span.start;
                        let s_end = offset + s.span.end;
                        json!({
                            "type": "SpreadElement",
                            "start": s_start,
                            "end": s_end,
                            "argument": estree_expr(&s.argument, source, offset)
                        })
                    }
                }
            }).collect();
            json!({
                "type": "ObjectExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "properties": properties
            })
        }
        Expression::ArrowFunctionExpression(arrow) => {
            let start = offset + arrow.span.start;
            let end = offset + arrow.span.end;
            let params: Vec<Value> = arrow.params.items.iter().map(|p| {
                estree_binding_pattern(p, source, offset)
            }).collect();
            let body = if arrow.expression {
                let stmts = &arrow.body.statements;
                if let Some(stmt) = stmts.first() {
                    if let oxc::ast::ast::Statement::ExpressionStatement(es) = stmt {
                        estree_expr(&es.expression, source, offset)
                    } else {
                        json!(null)
                    }
                } else {
                    json!(null)
                }
            } else {
                json!({
                    "type": "BlockStatement",
                    "start": offset + arrow.body.span.start,
                    "end": offset + arrow.body.span.end,
                    "body": []
                })
            };
            json!({
                "type": "ArrowFunctionExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "id": null,
                "expression": arrow.expression,
                "generator": false,
                "async": arrow.r#async,
                "params": params,
                "body": body
            })
        }
        Expression::AssignmentExpression(assign) => {
            let start = offset + assign.span.start;
            let end = offset + assign.span.end;
            json!({
                "type": "AssignmentExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "operator": assign.operator.as_str(),
                "left": estree_assignment_target(&assign.left, source, offset),
                "right": estree_expr(&assign.right, source, offset)
            })
        }
        Expression::UpdateExpression(upd) => {
            let start = offset + upd.span.start;
            let end = offset + upd.span.end;
            json!({
                "type": "UpdateExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "operator": upd.operator.as_str(),
                "argument": estree_simple_assign_target(&upd.argument, source, offset),
                "prefix": upd.prefix
            })
        }
        Expression::SequenceExpression(seq) => {
            let start = offset + seq.span.start;
            let end = offset + seq.span.end;
            let exprs: Vec<Value> = seq.expressions.iter().map(|e| estree_expr(e, source, offset)).collect();
            json!({
                "type": "SequenceExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "expressions": exprs
            })
        }
        Expression::ParenthesizedExpression(paren) => {
            // Parenthesized expressions aren't separate estree nodes
            estree_expr(&paren.expression, source, offset)
        }
        Expression::ThisExpression(this) => {
            let start = offset + this.span.start;
            let end = offset + this.span.end;
            json!({
                "type": "ThisExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end)
            })
        }
        Expression::AwaitExpression(aw) => {
            let start = offset + aw.span.start;
            let end = offset + aw.span.end;
            json!({
                "type": "AwaitExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "argument": estree_expr(&aw.argument, source, offset)
            })
        }
        Expression::YieldExpression(y) => {
            let start = offset + y.span.start;
            let end = offset + y.span.end;
            json!({
                "type": "YieldExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "argument": y.argument.as_ref().map(|a| estree_expr(a, source, offset)),
                "delegate": y.delegate
            })
        }
        Expression::NewExpression(n) => {
            let start = offset + n.span.start;
            let end = offset + n.span.end;
            json!({
                "type": "NewExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "callee": estree_expr(&n.callee, source, offset),
                "arguments": n.arguments.iter().map(|a| {
                    match a {
                        oxc::ast::ast::Argument::SpreadElement(s) => json!({
                            "type": "SpreadElement",
                            "start": offset + s.span.start,
                            "end": offset + s.span.end,
                            "argument": estree_expr(&s.argument, source, offset)
                        }),
                        _ => estree_expr(a.as_expression().unwrap(), source, offset)
                    }
                }).collect::<Vec<_>>()
            })
        }
        Expression::TaggedTemplateExpression(tte) => {
            let start = offset + tte.span.start;
            let end = offset + tte.span.end;
            let tl = &tte.quasi;
            let tl_start = offset + tl.span.start;
            let tl_end = offset + tl.span.end;
            let quasis: Vec<Value> = tl.quasis.iter().map(|q| {
                let q_start = offset + q.span.start;
                let q_end = offset + q.span.end;
                json!({
                    "type": "TemplateElement",
                    "start": q_start,
                    "end": q_end,
                    "value": {
                        "raw": q.value.raw.as_str(),
                        "cooked": q.value.cooked.as_ref().map(|c| c.as_str())
                    },
                    "tail": q.tail
                })
            }).collect();
            let exprs: Vec<Value> = tl.expressions.iter().map(|e| {
                estree_expr(e, source, offset)
            }).collect();
            json!({
                "type": "TaggedTemplateExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "tag": estree_expr(&tte.tag, source, offset),
                "quasi": {
                    "type": "TemplateLiteral",
                    "start": tl_start,
                    "end": tl_end,
                    "expressions": exprs,
                    "quasis": quasis
                }
            })
        }
        // Fallback for unsupported expression types
        _ => {
            json!({
                "type": "UnknownExpression",
                "raw": "unsupported"
            })
        }
    }
}

fn estree_expr_from_callee(callee: &oxc::ast::ast::Expression<'_>, source: &str, offset: u32) -> Value {
    estree_expr(callee, source, offset)
}

fn estree_property_key(key: &oxc::ast::ast::PropertyKey<'_>, source: &str, offset: u32) -> Value {
    match key {
        oxc::ast::ast::PropertyKey::StaticIdentifier(ident) => {
            let start = offset + ident.span.start;
            let end = offset + ident.span.end;
            json!({
                "type": "Identifier",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "name": ident.name.as_str()
            })
        }
        oxc::ast::ast::PropertyKey::StringLiteral(lit) => {
            let start = offset + lit.span.start;
            let end = offset + lit.span.end;
            json!({
                "type": "Literal",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "value": lit.value.as_str(),
                "raw": &source[start as usize..end as usize]
            })
        }
        oxc::ast::ast::PropertyKey::NumericLiteral(lit) => {
            let start = offset + lit.span.start;
            let end = offset + lit.span.end;
            json!({
                "type": "Literal",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "value": lit.value,
                "raw": &source[start as usize..end as usize]
            })
        }
        _ => {
            estree_expr(key.as_expression().unwrap(), source, offset)
        }
    }
}

fn estree_binding_pattern(pattern: &oxc::ast::ast::FormalParameter<'_>, source: &str, offset: u32) -> Value {
    estree_binding_pat(&pattern.pattern, source, offset)
}

fn estree_binding_pat(pat: &oxc::ast::ast::BindingPattern<'_>, source: &str, offset: u32) -> Value {
    match pat {
        oxc::ast::ast::BindingPattern::BindingIdentifier(ident) => {
            let start = offset + ident.span.start;
            let end = offset + ident.span.end;
            json!({
                "type": "Identifier",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "name": ident.name.as_str()
            })
        }
        oxc::ast::ast::BindingPattern::ObjectPattern(obj) => {
            let start = offset + obj.span.start;
            let end = offset + obj.span.end;
            let properties: Vec<Value> = obj.properties.iter().map(|p| {
                let p_start = offset + p.span.start;
                let p_end = offset + p.span.end;
                let key = estree_property_key(&p.key, source, offset);
                let value = estree_binding_pat(&p.value, source, offset);
                json!({
                    "type": "Property",
                    "start": p_start,
                    "end": p_end,
                    "method": false,
                    "shorthand": p.shorthand,
                    "computed": p.computed,
                    "key": key,
                    "value": value,
                    "kind": "init"
                })
            }).collect();
            json!({
                "type": "ObjectPattern",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "properties": properties
            })
        }
        oxc::ast::ast::BindingPattern::ArrayPattern(arr) => {
            let start = offset + arr.span.start;
            let end = offset + arr.span.end;
            let elements: Vec<Value> = arr.elements.iter().map(|el| {
                match el {
                    Some(pat) => estree_binding_pat(pat, source, offset),
                    None => Value::Null,
                }
            }).collect();
            json!({
                "type": "ArrayPattern",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "elements": elements
            })
        }
        oxc::ast::ast::BindingPattern::AssignmentPattern(assign) => {
            let start = offset + assign.span.start;
            let end = offset + assign.span.end;
            json!({
                "type": "AssignmentPattern",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "left": estree_binding_pat(&assign.left, source, offset),
                "right": estree_expr(&assign.right, source, offset)
            })
        }
    }
}

fn estree_assignment_target(target: &oxc::ast::ast::AssignmentTarget<'_>, source: &str, offset: u32) -> Value {
    match target {
        oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(ident) => {
            let start = offset + ident.span.start;
            let end = offset + ident.span.end;
            json!({
                "type": "Identifier",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "name": ident.name.as_str()
            })
        }
        _ => json!({ "type": "UnknownTarget" })
    }
}

fn estree_simple_assign_target(target: &oxc::ast::ast::SimpleAssignmentTarget<'_>, source: &str, offset: u32) -> Value {
    match target {
        oxc::ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
            let start = offset + ident.span.start;
            let end = offset + ident.span.end;
            json!({
                "type": "Identifier",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "name": ident.name.as_str()
            })
        }
        _ => json!({ "type": "UnknownTarget" })
    }
}

/// Filter out whitespace-only text nodes from block content.
fn filter_whitespace_nodes(nodes: &[TemplateNode]) -> Vec<&TemplateNode> {
    nodes.iter()
        .filter(|n| {
            if let TemplateNode::Text(t) = n {
                !t.data.chars().all(|c| c.is_ascii_whitespace())
            } else {
                true
            }
        })
        .collect()
}

/// For root fragments: only strip trailing whitespace-only text nodes.
fn strip_trailing_whitespace(nodes: &[TemplateNode]) -> Vec<&TemplateNode> {
    let mut result: Vec<&TemplateNode> = nodes.iter().collect();
    while let Some(last) = result.last() {
        if let TemplateNode::Text(t) = last {
            if t.data.chars().all(|c| c.is_ascii_whitespace()) {
                result.pop();
                continue;
            }
        }
        break;
    }
    result
}

/// Get the span start of a node.
fn node_span_start(node: &TemplateNode) -> u32 {
    match node {
        TemplateNode::Text(t) => t.span.start,
        TemplateNode::Element(e) => e.span.start,
        TemplateNode::Comment(c) => c.span.start,
        TemplateNode::IfBlock(b) => b.span.start,
        TemplateNode::EachBlock(b) => b.span.start,
        TemplateNode::AwaitBlock(b) => b.span.start,
        TemplateNode::KeyBlock(b) => b.span.start,
        TemplateNode::SnippetBlock(b) => b.span.start,
        TemplateNode::MustacheTag(m) => m.span.start,
        TemplateNode::RawMustacheTag(r) => r.span.start,
        TemplateNode::DebugTag(d) => d.span.start,
        TemplateNode::ConstTag(c) => c.span.start,
        TemplateNode::RenderTag(r) => r.span.start,
    }
}

/// Get the span end of a node.
fn node_span_end(node: &TemplateNode) -> u32 {
    match node {
        TemplateNode::Text(t) => t.span.end,
        TemplateNode::Element(e) => e.span.end,
        TemplateNode::Comment(c) => c.span.end,
        TemplateNode::IfBlock(b) => b.span.end,
        TemplateNode::EachBlock(b) => b.span.end,
        TemplateNode::AwaitBlock(b) => b.span.end,
        TemplateNode::KeyBlock(b) => b.span.end,
        TemplateNode::SnippetBlock(b) => b.span.end,
        TemplateNode::MustacheTag(m) => m.span.end,
        TemplateNode::RawMustacheTag(r) => r.span.end,
        TemplateNode::DebugTag(d) => d.span.end,
        TemplateNode::ConstTag(c) => c.span.end,
        TemplateNode::RenderTag(r) => r.span.end,
    }
}

/// Serialize filtered children nodes, returning (children_json, effective_end).
fn serialize_filtered_children(nodes: &[TemplateNode], source: &str, default_end: u32) -> (Vec<Value>, u32) {
    let filtered = filter_whitespace_nodes(nodes);
    let children: Vec<Value> = filtered.iter().map(|n| serialize_node_legacy(n, source)).collect();
    let end = filtered.last().map(|n| node_span_end(n)).unwrap_or(default_end);
    (children, end)
}

/// Decode HTML entities in text.
/// Only decodes entities with proper semicolons (e.g., &amp; but not &amp without semicolon).
fn decode_entities(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            // Collect the entity name
            let mut entity = String::new();
            entity.push('&');
            let mut found_semi = false;
            while let Some(&next) = chars.peek() {
                entity.push(next);
                chars.next();
                if next == ';' {
                    found_semi = true;
                    break;
                }
                if !next.is_alphanumeric() && next != '#' && next != 'x' && next != 'X' {
                    break;
                }
                if entity.len() > 10 {
                    break;
                }
            }
            if found_semi {
                match entity.as_str() {
                    "&amp;" => result.push('&'),
                    "&lt;" => result.push('<'),
                    "&gt;" => result.push('>'),
                    "&quot;" => result.push('"'),
                    "&#39;" => result.push('\''),
                    "&apos;" => result.push('\''),
                    "&nbsp;" => result.push('\u{00A0}'),
                    "&#x27;" => result.push('\''),
                    "&#x2F;" => result.push('/'),
                    "&#60;" => result.push('<'),
                    "&#62;" => result.push('>'),
                    _ => result.push_str(&entity),
                }
            } else {
                // Try to decode without semicolon (legacy HTML behavior)
                // Decode known named entities if the collected text exactly matches
                let entity_name = &entity[1..]; // strip leading &
                match entity_name {
                    "amp" | "lt" | "gt" | "quot" | "apos" | "nbsp" => {
                        match entity_name {
                            "amp" => result.push('&'),
                            "lt" => result.push('<'),
                            "gt" => result.push('>'),
                            "quot" => result.push('"'),
                            "apos" => result.push('\''),
                            "nbsp" => result.push('\u{00A0}'),
                            _ => unreachable!(),
                        }
                    }
                    _ => {
                        // Check if entity starts with a known name followed by non-alnum
                        let mut decoded = false;
                        for known in &["amp", "lt", "gt", "quot", "apos", "nbsp"] {
                            if entity_name.starts_with(known) {
                                let rest = &entity_name[known.len()..];
                                if !rest.is_empty() && !rest.starts_with(|c: char| c.is_alphanumeric()) {
                                    match *known {
                                        "amp" => result.push('&'),
                                        "lt" => result.push('<'),
                                        "gt" => result.push('>'),
                                        "quot" => result.push('"'),
                                        "apos" => result.push('\''),
                                        "nbsp" => result.push('\u{00A0}'),
                                        _ => {}
                                    }
                                    result.push_str(rest);
                                    decoded = true;
                                    break;
                                }
                            }
                        }
                        if !decoded {
                            result.push_str(&entity);
                        }
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Serialize a `SvelteAst` to the legacy Svelte JSON format.
pub fn to_legacy_json(ast: &SvelteAst, source: &str) -> Value {
    let html = serialize_fragment_legacy(&ast.html, source);
    let mut root = json!({ "html": html });

    // Add css if present
    if let Some(style) = &ast.css {
        root["css"] = serialize_css_legacy(style, source);
    }

    // Add instance script if present
    if let Some(script) = &ast.instance {
        root["instance"] = serialize_script_legacy(script, source, "default");
    }

    // Add module script if present
    if let Some(script) = &ast.module {
        root["module"] = serialize_script_legacy(script, source, "module");
    }

    root
}

fn serialize_css_legacy(style: &Style, source: &str) -> Value {
    // Basic CSS serialization - just the raw content and span
    json!({
        "type": "Style",
        "start": style.span.start,
        "end": style.span.end,
        "attributes": [],
        "children": [],
        "content": {
            "start": style.span.start + 7 + style.lang.as_ref().map(|l| l.len() + 7).unwrap_or(0) as u32,
            "end": style.span.end - 8,
            "styles": style.content
        }
    })
}

fn serialize_script_legacy(script: &Script, source: &str, context: &str) -> Value {
    // Parse the script content with oxc and serialize to estree
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let alloc = Allocator::default();
    let source_type = if script.lang.as_deref() == Some("ts") {
        SourceType::ts()
    } else {
        SourceType::mjs()
    };

    // Find the script content start position in the original source
    let tag_text = &source[script.span.start as usize..script.span.end as usize];
    let content_start_rel = tag_text.find('>').map(|p| p + 1).unwrap_or(0);
    let content_start = script.span.start + content_start_rel as u32;

    let result = Parser::new(&alloc, &script.content, source_type).parse();

    let program_end = content_start + script.content.len() as u32;

    // Compute loc: start is {line:1, column:0} (script-relative convention)
    // End uses full source coordinates for the closing </script> position
    let (end_line, end_col) = offset_to_loc(source, script.span.end as usize);
    let content_loc = json!({
        "start": { "line": 1, "column": 0 },
        "end": { "line": end_line, "column": end_col }
    });

    // Serialize the program body statements
    let body: Vec<Value> = result.program.body.iter().map(|stmt| {
        serialize_statement_legacy(stmt, source, content_start)
    }).collect();

    json!({
        "type": "Script",
        "start": script.span.start,
        "end": script.span.end,
        "context": context,
        "content": {
            "type": "Program",
            "start": content_start,
            "end": program_end,
            "loc": content_loc,
            "body": body,
            "sourceType": "module"
        }
    })
}

fn offset_to_loc_json(text: &str, offset: usize) -> Value {
    let (line, col) = offset_to_loc(text, offset);
    json!({ "line": line, "column": col })
}

/// Serialize a JS statement to legacy estree JSON.
fn serialize_statement_legacy(stmt: &oxc::ast::ast::Statement<'_>, source: &str, offset: u32) -> Value {
    use oxc::ast::ast::Statement;
    match stmt {
        Statement::VariableDeclaration(decl) => {
            let start = offset + decl.span.start;
            let end = offset + decl.span.end;
            let declarations: Vec<Value> = decl.declarations.iter().map(|d| {
                let d_start = offset + d.span.start;
                let d_end = offset + d.span.end;
                let id = estree_binding_pat(&d.id, source, offset);
                let init = d.init.as_ref().map(|e| estree_expr(e, source, offset));
                let mut obj = json!({
                    "type": "VariableDeclarator",
                    "start": d_start,
                    "end": d_end,
                    "loc": loc_json(source, d_start, d_end),
                    "id": id,
                    "init": init
                });
                obj
            }).collect();
            json!({
                "type": "VariableDeclaration",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "declarations": declarations,
                "kind": match decl.kind {
                    oxc::ast::ast::VariableDeclarationKind::Var => "var",
                    oxc::ast::ast::VariableDeclarationKind::Let => "let",
                    oxc::ast::ast::VariableDeclarationKind::Const => "const",
                    oxc::ast::ast::VariableDeclarationKind::Using => "using",
                    oxc::ast::ast::VariableDeclarationKind::AwaitUsing => "await using",
                }
            })
        }
        Statement::ExpressionStatement(es) => {
            let start = offset + es.span.start;
            let end = offset + es.span.end;
            json!({
                "type": "ExpressionStatement",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "expression": estree_expr(&es.expression, source, offset)
            })
        }
        Statement::ImportDeclaration(imp) => {
            let start = offset + imp.span.start;
            let end = offset + imp.span.end;
            json!({
                "type": "ImportDeclaration",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end)
            })
        }
        Statement::ExportNamedDeclaration(exp) => {
            let start = offset + exp.span.start;
            let end = offset + exp.span.end;
            let declaration = exp.declaration.as_ref().map(|d| {
                serialize_statement_legacy_from_decl(d, source, offset)
            });
            json!({
                "type": "ExportNamedDeclaration",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "declaration": declaration,
                "specifiers": [],
                "source": null
            })
        }
        _ => {
            // Fallback for unhandled statement types
            json!({
                "type": "UnknownStatement"
            })
        }
    }
}

fn serialize_statement_legacy_from_decl(decl: &oxc::ast::ast::Declaration<'_>, source: &str, offset: u32) -> Value {
    use oxc::ast::ast::Declaration;
    match decl {
        Declaration::VariableDeclaration(v) => {
            // Reuse the VariableDeclaration serialization by wrapping in a Statement
            let start = offset + v.span.start;
            let end = offset + v.span.end;
            let declarations: Vec<Value> = v.declarations.iter().map(|d| {
                let d_start = offset + d.span.start;
                let d_end = offset + d.span.end;
                let id = estree_binding_pat(&d.id, source, offset);
                let init = d.init.as_ref().map(|e| estree_expr(e, source, offset));
                json!({
                    "type": "VariableDeclarator",
                    "start": d_start,
                    "end": d_end,
                    "loc": loc_json(source, d_start, d_end),
                    "id": id,
                    "init": init
                })
            }).collect();
            json!({
                "type": "VariableDeclaration",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "declarations": declarations,
                "kind": match v.kind {
                    oxc::ast::ast::VariableDeclarationKind::Var => "var",
                    oxc::ast::ast::VariableDeclarationKind::Let => "let",
                    oxc::ast::ast::VariableDeclarationKind::Const => "const",
                    oxc::ast::ast::VariableDeclarationKind::Using => "using",
                    oxc::ast::ast::VariableDeclarationKind::AwaitUsing => "await using",
                }
            })
        }
        Declaration::FunctionDeclaration(f) => {
            let start = offset + f.span.start;
            let end = offset + f.span.end;
            json!({
                "type": "FunctionDeclaration",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "id": f.id.as_ref().map(|id| {
                    let id_start = offset + id.span.start;
                    let id_end = offset + id.span.end;
                    json!({
                        "type": "Identifier",
                        "start": id_start,
                        "end": id_end,
                        "loc": loc_json(source, id_start, id_end),
                        "name": id.name.as_str()
                    })
                })
            })
        }
        _ => json!({ "type": "UnknownDeclaration" })
    }
}

fn serialize_fragment_legacy(fragment: &Fragment, source: &str) -> Value {
    // Root fragment: only strip trailing whitespace, keep all other nodes
    let filtered = strip_trailing_whitespace(&fragment.nodes);
    let children: Vec<Value> = filtered.iter().map(|n| serialize_node_legacy(n, source)).collect();
    let end = filtered.last().map(|n| node_span_end(n)).unwrap_or(fragment.span.end);
    // Fragment start: use the first non-whitespace-text node's start position
    let start = filtered.iter()
        .find(|n| !matches!(n, TemplateNode::Text(t) if t.data.chars().all(|c| c.is_ascii_whitespace())))
        .map(|n| node_span_start(n))
        .or_else(|| filtered.first().map(|n| node_span_start(n)))
        .unwrap_or(fragment.span.start);
    json!({
        "type": "Fragment",
        "start": start,
        "end": end,
        "children": children
    })
}

fn serialize_node_legacy(node: &TemplateNode, source: &str) -> Value {
    match node {
        TemplateNode::Text(t) => {
            json!({
                "type": "Text",
                "start": t.span.start,
                "end": t.span.end,
                "raw": t.data,
                "data": decode_entities(&t.data)
            })
        }
        TemplateNode::Comment(c) => {
            // Parse svelte-ignore directives from comment text
            let ignores: Vec<&str> = if c.data.trim_start().starts_with("svelte-ignore") {
                let after_prefix = c.data.trim_start().strip_prefix("svelte-ignore").unwrap_or("");
                after_prefix.split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                Vec::new()
            };
            json!({
                "type": "Comment",
                "start": c.span.start,
                "end": c.span.end,
                "data": c.data,
                "ignores": ignores
            })
        }
        TemplateNode::Element(el) => {
            let children: Vec<Value> = el.children.iter().map(|n| serialize_node_legacy(n, source)).collect();
            // For svelte:component, extract the `this` attribute as `expression`
            let mut extra_fields = serde_json::Map::new();
            let mut filtered_attrs = el.attributes.clone();
            if el.name == "svelte:component" || el.name == "svelte:element" {
                if let Some(idx) = filtered_attrs.iter().position(|a| {
                    matches!(a, Attribute::NormalAttribute { name, .. } if name == "this")
                }) {
                    let this_attr = filtered_attrs.remove(idx);
                    if let Attribute::NormalAttribute { value, span, .. } = &this_attr {
                        match value {
                            AttributeValue::Expression(expr) => {
                                let region = &source[span.start as usize..span.end as usize];
                                let brace_pos = region.find('{').map(|p| p + 1).unwrap_or(0);
                                let expr_start = span.start + brace_pos as u32;
                                extra_fields.insert("expression".to_string(),
                                    expression_to_estree(source, expr.trim(), expr_start));
                            }
                            AttributeValue::Static(s) => {
                                let inner = s.trim();
                                if inner.starts_with('{') && inner.ends_with('}') {
                                    let expr_str = &inner[1..inner.len()-1];
                                    let region = &source[span.start as usize..span.end as usize];
                                    let brace_pos = region.find('{').map(|p| p + 1).unwrap_or(0);
                                    let expr_start = span.start + brace_pos as u32;
                                    extra_fields.insert("expression".to_string(),
                                        expression_to_estree(source, expr_str.trim(), expr_start));
                                }
                            }
                            AttributeValue::Concat(parts) => {
                                // Single expression in concat: ="{expr}"
                                if parts.len() == 1 {
                                    if let AttributeValuePart::Expression(expr) = &parts[0] {
                                        let region = &source[span.start as usize..span.end as usize];
                                        let brace_pos = region.find('{').map(|p| p + 1).unwrap_or(0);
                                        let expr_start = span.start + brace_pos as u32;
                                        extra_fields.insert("expression".to_string(),
                                            expression_to_estree(source, expr.trim(), expr_start));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            let el_type = if el.name.starts_with(|c: char| c.is_uppercase()) {
                "InlineComponent"
            } else if el.name.starts_with("svelte:") {
                match el.name.as_str() {
                    "svelte:self" => "InlineComponent",
                    "svelte:component" => "InlineComponent",
                    "svelte:element" => "InlineComponent",
                    "svelte:window" => "Window",
                    "svelte:document" => "Document",
                    "svelte:body" => "Body",
                    "svelte:head" => "Head",
                    "svelte:options" => "Options",
                    "svelte:fragment" => "SlotTemplate",
                    _ => "Element",
                }
            } else if el.name == "slot" {
                "Slot"
            } else {
                "Element"
            };
            let attributes: Vec<Value> = filtered_attrs.iter().map(|a| serialize_attribute_legacy(a, source)).collect();
            let mut obj = json!({
                "type": el_type,
                "start": el.span.start,
                "end": el.span.end,
                "name": el.name,
                "attributes": attributes,
                "children": children
            });
            for (key, val) in extra_fields {
                obj[key] = val;
            }
            obj
        }
        TemplateNode::MustacheTag(m) => {
            let expr_start = m.span.start + 1; // skip '{'
            json!({
                "type": "MustacheTag",
                "start": m.span.start,
                "end": m.span.end,
                "expression": expression_to_estree(source, m.expression.trim(), expr_start)
            })
        }
        TemplateNode::RawMustacheTag(r) => {
            // {@html expr} - expression starts after "{@html "
            let tag_text = &source[r.span.start as usize..r.span.end as usize];
            let expr_offset = tag_text.find(r.expression.trim_start()).unwrap_or(7);
            let expr_start = r.span.start + expr_offset as u32;
            json!({
                "type": "RawMustacheTag",
                "start": r.span.start,
                "end": r.span.end,
                "expression": expression_to_estree(source, r.expression.trim(), expr_start)
            })
        }
        TemplateNode::DebugTag(d) => {
            let idents: Vec<Value> = d.identifiers.iter().enumerate().map(|(_, ident)| {
                // Try to find the identifier position in source
                json!({
                    "type": "Identifier",
                    "name": ident
                })
            }).collect();
            json!({
                "type": "DebugTag",
                "start": d.span.start,
                "end": d.span.end,
                "identifiers": idents
            })
        }
        TemplateNode::ConstTag(c) => {
            json!({
                "type": "ConstTag",
                "start": c.span.start,
                "end": c.span.end,
                "declaration": c.declaration
            })
        }
        TemplateNode::RenderTag(r) => {
            let expr_start = r.span.start + 9; // skip "{@render "
            json!({
                "type": "RenderTag",
                "start": r.span.start,
                "end": r.span.end,
                "expression": expression_to_estree(source, r.expression.trim(), expr_start)
            })
        }
        TemplateNode::IfBlock(block) => {
            let (children, _) = serialize_filtered_children(&block.consequent.nodes, source, block.span.end);
            // For the condition expression, find it in source after "{#if "
            let expr_start = if block.test.is_empty() {
                block.span.start
            } else {
                // Find the expression in source
                let prefix_len = if source[block.span.start as usize..].starts_with("{#if") { 5 } else { 10 };
                block.span.start + prefix_len
            };
            let mut obj = json!({
                "type": "IfBlock",
                "start": block.span.start,
                "end": block.span.end,
                "expression": expression_to_estree(source, block.test.trim(), expr_start),
                "children": children
            });
            if let Some(alt) = &block.alternate {
                match alt.as_ref() {
                    TemplateNode::IfBlock(alt_block) => {
                        if alt_block.test.is_empty() {
                            // {:else} block - end is at the end of the fragment (before {/if})
                            let (else_children, _) = serialize_filtered_children(
                                &alt_block.consequent.nodes, source, alt_block.span.end
                            );
                            obj["else"] = json!({
                                "type": "ElseBlock",
                                "start": alt_block.span.start,
                                "end": alt_block.span.end,
                                "children": else_children
                            });
                        } else {
                            // {:else if ...} block — wrap IfBlock in an ElseBlock
                            // The IfBlock's span starts at {:else if, so we find the } to get content start
                            let tag_region = &source[alt_block.span.start as usize..];
                            let close_brace = tag_region.find('}').unwrap_or(0);
                            let content_start = alt_block.span.start + close_brace as u32 + 1;

                            let inner = serialize_elseif_block(alt_block, source, content_start, block.span.end);
                            // ElseBlock end should be the alt IfBlock's span end (where {/if} or next {:else} starts)
                            let else_block_end = alt_block.span.end;
                            obj["else"] = json!({
                                "type": "ElseBlock",
                                "start": content_start,
                                "end": else_block_end,
                                "children": [inner]
                            });
                        }
                    }
                    _ => {}
                }
            }
            obj
        }
        TemplateNode::EachBlock(block) => {
            let (children, _) = serialize_filtered_children(&block.body.nodes, source, block.span.end);
            let expr_start = block.span.start + 7; // skip "{#each "
            let context_str = &block.context;

            // Find context position in source after " as "
            let header = &source[block.span.start as usize..];
            let as_pos = header.find(" as ").map(|p| p + 4).unwrap_or(0);
            let ctx_start = block.span.start + as_pos as u32;
            let ctx_end = ctx_start + context_str.len() as u32;

            let context = json!({
                "type": "Identifier",
                "name": context_str,
                "start": ctx_start,
                "end": ctx_end,
                "loc": loc_json_with_char(source, ctx_start, ctx_end)
            });

            let mut obj = json!({
                "type": "EachBlock",
                "start": block.span.start,
                "end": block.span.end,
                "children": children,
                "context": context,
                "expression": expression_to_estree(source, block.expression.trim(), expr_start)
            });

            if let Some(idx) = &block.index {
                obj["index"] = json!(idx);
            }

            if let Some(key) = &block.key {
                // Find key expression position
                let header = &source[block.span.start as usize..];
                if let Some(paren) = header.find('(') {
                    let key_start = block.span.start + paren as u32 + 1;
                    obj["key"] = expression_to_estree(source, key.trim(), key_start);
                }
            }

            if let Some(fb) = &block.fallback {
                let (else_children, _) = serialize_filtered_children(&fb.nodes, source, fb.span.end);
                obj["else"] = json!({
                    "type": "ElseBlock",
                    "start": fb.span.start,
                    "end": fb.span.end,
                    "children": else_children
                });
            }

            obj
        }
        TemplateNode::AwaitBlock(block) => {
            let expr_start = block.span.start + 8; // skip "{#await "
            let mut obj = json!({
                "type": "AwaitBlock",
                "start": block.span.start,
                "end": block.span.end,
                "expression": expression_to_estree(source, block.expression.trim(), expr_start)
            });
            if let Some(pending) = &block.pending {
                let (children, _) = serialize_filtered_children(&pending.nodes, source, pending.span.end);
                obj["pending"] = json!({
                    "type": "PendingBlock",
                    "children": children
                });
            }
            if let Some(then) = &block.then {
                let (children, _) = serialize_filtered_children(&then.nodes, source, then.span.end);
                obj["then"] = json!({
                    "type": "ThenBlock",
                    "children": children
                });
                if let Some(binding) = &block.then_binding {
                    obj["value"] = json!(binding);
                }
            }
            if let Some(catch) = &block.catch {
                let (children, _) = serialize_filtered_children(&catch.nodes, source, catch.span.end);
                obj["catch"] = json!({
                    "type": "CatchBlock",
                    "children": children
                });
                if let Some(binding) = &block.catch_binding {
                    obj["error"] = json!(binding);
                }
            }
            obj
        }
        TemplateNode::KeyBlock(block) => {
            let (children, _) = serialize_filtered_children(&block.body.nodes, source, block.span.end);
            let expr_start = block.span.start + 6; // skip "{#key "
            json!({
                "type": "KeyBlock",
                "start": block.span.start,
                "end": block.span.end,
                "expression": expression_to_estree(source, block.expression.trim(), expr_start),
                "children": children
            })
        }
        TemplateNode::SnippetBlock(block) => {
            let (children, _) = serialize_filtered_children(&block.body.nodes, source, block.span.end);
            json!({
                "type": "SnippetBlock",
                "start": block.span.start,
                "end": block.span.end,
                "name": block.name,
                "params": block.params,
                "children": children
            })
        }
    }
}

/// Serialize an {:else if} IfBlock with elseif:true flag.
/// `outer_end` is the end position of the outermost {/if} tag.
fn serialize_elseif_block(block: &IfBlock, source: &str, content_start: u32, outer_end: u32) -> Value {
    let (children, _) = serialize_filtered_children(&block.consequent.nodes, source, block.span.end);

    // The expression is within the {:else if ...} tag, before content_start
    let tag_text = &source[block.span.start as usize..(content_start as usize).saturating_sub(1)];
    let else_if_prefix = "{:else if";
    let expr_offset = if let Some(idx) = tag_text.find(else_if_prefix) {
        idx + else_if_prefix.len()
    } else {
        0
    };
    // Skip whitespace
    let mut expr_start = block.span.start + expr_offset as u32;
    while (expr_start as usize) < source.len() && source.as_bytes()[expr_start as usize].is_ascii_whitespace() {
        expr_start += 1;
    }

    let mut obj = json!({
        "type": "IfBlock",
        "start": content_start,
        "end": outer_end,
        "expression": expression_to_estree(source, block.test.trim(), expr_start),
        "children": children,
        "elseif": true
    });

    // Handle nested alternates
    if let Some(alt) = &block.alternate {
        if let TemplateNode::IfBlock(alt_block) = alt.as_ref() {
            if alt_block.test.is_empty() {
                // {:else} block
                let (else_children, _) = serialize_filtered_children(
                    &alt_block.consequent.nodes, source, alt_block.span.end
                );
                obj["else"] = json!({
                    "type": "ElseBlock",
                    "start": alt_block.span.start,
                    "end": alt_block.span.end,
                    "children": else_children
                });
            } else {
                // Nested {:else if ...}
                let tag_region = &source[alt_block.span.start as usize..];
                let close_brace = tag_region.find('}').unwrap_or(0);
                let nested_content_start = alt_block.span.start + close_brace as u32 + 1;
                let inner = serialize_elseif_block(alt_block, source, nested_content_start, outer_end);
                let else_block_end = alt_block.span.end;
                obj["else"] = json!({
                    "type": "ElseBlock",
                    "start": nested_content_start,
                    "end": else_block_end,
                    "children": [inner]
                });
            }
        }
    }

    obj
}

fn serialize_attribute_legacy(attr: &Attribute, source: &str) -> Value {
    match attr {
        Attribute::NormalAttribute { name, value, span } => {
            // Check for shorthand attribute: {name}
            let tag_region = &source[span.start as usize..span.end as usize];
            let is_shorthand = tag_region.starts_with('{') && tag_region.ends_with('}')
                && matches!(value, AttributeValue::Expression(e) if e == name);

            if is_shorthand {
                // Shorthand: {id} → Attribute with AttributeShorthand value
                let expr_start = span.start + 1; // after {
                let expr_end = span.end - 1; // before }
                let name_loc = loc_json_with_char(source, expr_start, expr_end);
                json!({
                    "type": "Attribute",
                    "start": span.start,
                    "end": span.end,
                    "name": name,
                    "name_loc": name_loc,
                    "value": [{
                        "type": "AttributeShorthand",
                        "start": expr_start,
                        "end": expr_end,
                        "expression": {
                            "type": "Identifier",
                            "name": name,
                            "start": expr_start,
                            "end": expr_end,
                            "loc": loc_json_with_char(source, expr_start, expr_end)
                        }
                    }]
                })
            } else {
                let name_offset = tag_region.find(name.as_str()).unwrap_or(0);
                let n_start = span.start + name_offset as u32;
                let n_end = n_start + name.len() as u32;

                let value_json = serialize_attr_value_legacy(value, source, span);

                json!({
                    "type": "Attribute",
                    "start": span.start,
                    "end": span.end,
                    "name": name,
                    "name_loc": loc_json_with_char(source, n_start, n_end),
                    "value": value_json
                })
            }
        }
        Attribute::Spread { span } => {
            // The spread expression is between {... and }
            let region = &source[span.start as usize..span.end as usize];
            let expr_str = region.trim_start_matches('{').trim_start_matches("...").trim_end_matches('}');
            let expr_start_offset = region.find("...").map(|p| p + 3).unwrap_or(1);
            let expr_start = span.start + expr_start_offset as u32;
            json!({
                "type": "Spread",
                "start": span.start,
                "end": span.end,
                "expression": expression_to_estree(source, expr_str.trim(), expr_start)
            })
        }
        Attribute::Directive { kind, name, modifiers, span } => {
            let type_name = match kind {
                DirectiveKind::EventHandler => "EventHandler",
                DirectiveKind::Binding => "Binding",
                DirectiveKind::Class => "Class",
                DirectiveKind::StyleDirective => "StyleDirective",
                DirectiveKind::Use => "Action",
                DirectiveKind::Transition => "Transition",
                DirectiveKind::In => "Transition",
                DirectiveKind::Out => "Transition",
                DirectiveKind::Animate => "Animation",
                DirectiveKind::Let => "Let",
            };

            // Calculate name_loc: from directive start to end of directive name (prefix:name)
            let attr_text = &source[span.start as usize..span.end as usize];
            let colon_pos = attr_text.find(':').unwrap_or(0);
            let name_start = span.start;
            // name_loc covers the entire "prefix:name" part
            let name_end_rel = if let Some(eq) = attr_text.find('=') {
                eq
            } else if let Some(pipe) = attr_text.find('|') {
                pipe
            } else {
                attr_text.len()
            };
            let name_end = span.start + name_end_rel as u32;

            // Parse expression from directive value if present
            let expression = if let Some(eq_pos) = attr_text.find('=') {
                let value_part = attr_text[eq_pos + 1..].trim_start();
                if value_part.starts_with('{') && value_part.ends_with('}') {
                    // Direct expression: ={expr}
                    let expr_str = &value_part[1..value_part.len()-1];
                    let brace_pos = attr_text[eq_pos..].find('{').unwrap_or(1);
                    let expr_start = span.start + eq_pos as u32 + brace_pos as u32 + 1;
                    Some(expression_to_estree(source, expr_str.trim(), expr_start))
                } else if (value_part.starts_with('"') || value_part.starts_with('\''))
                    && value_part.len() > 2
                {
                    // Quoted value: ="{expr}" or ="static"
                    let inner = &value_part[1..value_part.len()-1];
                    if inner.starts_with('{') && inner.ends_with('}') {
                        // Quoted expression: ="{expr}"
                        let expr_str = &inner[1..inner.len()-1];
                        let brace_pos = attr_text[eq_pos..].find('{').unwrap_or(2);
                        let expr_start = span.start + eq_pos as u32 + brace_pos as u32 + 1;
                        Some(expression_to_estree(source, expr_str.trim(), expr_start))
                    } else {
                        // Static string value
                        let val_start_rel = attr_text[eq_pos..].find(|c: char| c == '"' || c == '\'').unwrap_or(1);
                        let val_start = span.start + eq_pos as u32 + val_start_rel as u32 + 1;
                        let val_end = span.end - 1;
                        Some(json!([{
                            "type": "Text",
                            "start": val_start,
                            "end": val_end,
                            "raw": inner,
                            "data": inner
                        }]))
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let mut obj = json!({
                "start": span.start,
                "end": span.end,
                "type": type_name,
                "name": name,
                "name_loc": loc_json_with_char(source, name_start, name_end),
                "modifiers": modifiers
            });

            // Style directives use "value" instead of "expression"
            if matches!(kind, DirectiveKind::StyleDirective) {
                if let Some(expr) = expression {
                    // Wrap in a MustacheTag-like array for style directives
                    let brace_pos = attr_text.find('{');
                    let close_brace = attr_text.rfind('}');
                    if let (Some(bp), Some(cbp)) = (brace_pos, close_brace) {
                        let mustache_start = span.start + bp as u32;
                        let mustache_end = span.start + cbp as u32 + 1;
                        obj["value"] = json!([{
                            "type": "MustacheTag",
                            "start": mustache_start,
                            "end": mustache_end,
                            "expression": expr
                        }]);
                    } else {
                        obj["value"] = json!([expr]);
                    }
                } else {
                    obj["value"] = json!(true);
                }
            } else {
                if let Some(expr) = expression {
                    obj["expression"] = expr;
                } else if matches!(kind, DirectiveKind::Binding) {
                    // Shorthand binding: bind:foo → expression is Identifier("foo")
                    // Find the name position after the colon
                    let colon_pos = attr_text.find(':').unwrap_or(0);
                    let name_abs_start = span.start + colon_pos as u32 + 1;
                    let name_abs_end = name_end;
                    obj["expression"] = json!({
                        "type": "Identifier",
                        "start": name_abs_start,
                        "end": name_abs_end,
                        "name": name
                    });
                } else {
                    obj["expression"] = Value::Null;
                }
            }

            // Add intro/outro for transitions
            match kind {
                DirectiveKind::Transition => {
                    obj["intro"] = json!(true);
                    obj["outro"] = json!(true);
                }
                DirectiveKind::In => {
                    obj["intro"] = json!(true);
                    obj["outro"] = json!(false);
                }
                DirectiveKind::Out => {
                    obj["intro"] = json!(false);
                    obj["outro"] = json!(true);
                }
                _ => {}
            }

            obj
        }
    }
}

fn serialize_attr_value_legacy(value: &AttributeValue, source: &str, attr_span: &oxc::span::Span) -> Value {
    match value {
        AttributeValue::True => json!(true),
        AttributeValue::Static(s) => {
            // Find the value position in source
            let region = &source[attr_span.start as usize..attr_span.end as usize];
            if s.is_empty() {
                // Empty string: ="", position between the quotes
                let quote_pos = region.rfind(|c: char| c == '"' || c == '\'').unwrap_or(region.len());
                let val_pos = attr_span.start + quote_pos as u32;
                json!([{
                    "start": val_pos,
                    "end": val_pos,
                    "type": "Text",
                    "raw": "",
                    "data": ""
                }])
            } else {
                let val_start_rel = region.find(s.as_str()).unwrap_or(0);
                let val_start = attr_span.start + val_start_rel as u32;
                let val_end = val_start + s.len() as u32;
                json!([{
                    "start": val_start,
                    "end": val_end,
                    "type": "Text",
                    "raw": s,
                    "data": decode_entities(s)
                }])
            }
        }
        AttributeValue::Expression(expr) => {
            // Find expression position - after ={
            let region = &source[attr_span.start as usize..attr_span.end as usize];
            let expr_start_rel = region.find('{').map(|p| p + 1).unwrap_or(0);
            let expr_start = attr_span.start + expr_start_rel as u32;
            // The overall mustache tag span includes the { }
            let mustache_start = attr_span.start + region.find('{').unwrap_or(0) as u32;
            let mustache_end = attr_span.start + region.rfind('}').map(|p| p + 1).unwrap_or(region.len()) as u32;
            json!([{
                "type": "MustacheTag",
                "start": mustache_start,
                "end": mustache_end,
                "expression": expression_to_estree(source, expr.trim(), expr_start)
            }])
        }
        AttributeValue::Concat(parts) => {
            let values: Vec<Value> = parts.iter().map(|part| {
                match part {
                    AttributeValuePart::Static(s) => json!({
                        "type": "Text",
                        "raw": s,
                        "data": decode_entities(s)
                    }),
                    AttributeValuePart::Expression(expr) => json!({
                        "type": "MustacheTag",
                        "expression": expression_to_estree(source, expr.trim(), 0)
                    }),
                }
            }).collect();
            Value::Array(values)
        }
    }
}
