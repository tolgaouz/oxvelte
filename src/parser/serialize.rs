//! Serialize oxvelte AST to the Svelte compiler's legacy JSON format.
//!
//! This module converts our internal AST representation into `serde_json::Value`
//! matching the expected output from the Svelte 4 compiler's parser, so we can
//! compare against the test fixtures in `fixtures/parser/legacy/`.

use crate::ast::*;
use serde_json::{json, Value};

/// Convert a byte offset to a UTF-16 code unit offset (JavaScript string position).
fn byte_to_char_offset(source: &str, byte_offset: usize) -> usize {
    let mut utf16_offset = 0;
    for ch in source[..byte_offset.min(source.len())].chars() {
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

/// Check if source contains multi-byte characters.
fn has_multibyte(source: &str) -> bool {
    source.len() != source.chars().count()
}

fn trimmed_span_start(span: oxc::span::Span, raw: &str) -> u32 {
    span.start + (raw.len() - raw.trim_start().len()) as u32
}

fn index_after_next_closing_brace(source: &str, offset: u32) -> u32 {
    let start = (offset as usize).min(source.len());
    source[start..]
        .find('}')
        .map(|idx| (start + idx + 1) as u32)
        .unwrap_or(offset)
}

fn index_after_last_closing_brace_at_or_before(source: &str, offset: u32) -> u32 {
    if source.is_empty() {
        return 0;
    }

    let end = (offset as usize).min(source.len().saturating_sub(1));
    source[..=end]
        .rfind('}')
        .map(|idx| (idx + 1) as u32)
        .unwrap_or(0)
}

fn source_for_span<'a>(source: &'a str, span: oxc::span::Span) -> &'a str {
    source
        .get(span.start as usize..span.end as usize)
        .unwrap_or("")
}

/// Compute line/column location info from a byte offset in source text.
/// Line numbers are 1-based, columns are 0-based.
fn offset_to_loc(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col_utf16: usize = 0;
    let mut line_start_byte: usize = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start_byte = i + 1;
            col_utf16 = 0;
        } else {
            col_utf16 += ch.len_utf16();
        }
    }
    // If source has multi-byte chars, use UTF-16 column; otherwise use byte column
    if has_multibyte(source) {
        (line, col_utf16)
    } else {
        let col = offset.saturating_sub(line_start_byte);
        (line, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_script_braced_attribute_value_stays_static_text() {
        let source = "<script lang={ts}>\n</script>";
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);

        let value = &actual["instance"]["attributes"][0]["value"][0];
        assert_eq!(value["type"], "Text");
        assert_eq!(value["raw"], "{ts}");
        assert_eq!(value["data"], "{ts}");
    }

    #[test]
    fn each_context_and_key_offsets_come_from_header_spans() {
        let source = r#"{#each labels.includes(" as ") as label (label.id)}{/each}"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);
        let each = &actual["fragment"]["nodes"][0];

        let context_start = source.rfind("as label").unwrap() + "as ".len();
        let key_start = source.rfind("(label.id)").unwrap() + 1;
        assert_eq!(each["context"]["start"], context_start as u32);
        assert_eq!(each["key"]["start"], key_start as u32);
    }

    #[test]
    fn modern_each_block_serializes_fallback_fragment() {
        let source = "{#each items as item}<p>{item}</p>{:else}<p>empty</p>{/each}";
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);
        let fallback = &actual["fragment"]["nodes"][0]["fallback"];

        assert_eq!(fallback["type"], "Fragment");
        assert_eq!(fallback["nodes"][0]["type"], "RegularElement");
        assert_eq!(fallback["nodes"][0]["start"], 41);
    }

    #[test]
    fn head_title_uses_dedicated_modern_and_legacy_types() {
        let source = "<svelte:head>{#if show}<title>Hello</title>{/if}</svelte:head>";
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);

        let modern = to_modern_json(&result.ast, source);
        let modern_title =
            &modern["fragment"]["nodes"][0]["fragment"]["nodes"][0]["consequent"]["nodes"][0];
        assert_eq!(modern_title["type"], "TitleElement");

        let legacy = to_legacy_json(&result.ast, source);
        let legacy_title = &legacy["html"]["children"][0]["children"][0]["children"][0];
        assert_eq!(legacy_title["type"], "Title");
    }

    #[test]
    fn await_shorthand_binding_offset_ignores_keyword_inside_expression() {
        let source = r#"{#await message.includes(" then ") then message}{/await}"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);
        let await_block = &actual["fragment"]["nodes"][0];

        let binding_start = source.rfind("then message").unwrap() + "then ".len();
        assert_eq!(await_block["value"]["start"], binding_start as u32);
    }

    #[test]
    fn legacy_await_catch_before_then_spans_follow_svelte_adapter() {
        let source = "{#await p}<p>pending</p>{:catch e}<p>err</p>{:then v}<p>ok</p>{/await}";
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_legacy_json(&result.ast, source);
        let await_block = &actual["html"]["children"][0];

        assert_eq!(await_block["then"]["start"], 24);
        assert_eq!(await_block["then"]["end"], 62);
        assert_eq!(await_block["catch"]["start"], 62);
        assert_eq!(await_block["catch"]["end"], 44);
    }

    #[test]
    fn legacy_await_empty_then_before_catch_uses_previous_clause_end() {
        let source = "{#await p}<p>pending</p>{:then v}{:catch e}<p>err</p>{/await}";
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_legacy_json(&result.ast, source);
        let await_block = &actual["html"]["children"][0];

        assert_eq!(await_block["then"]["start"], 24);
        assert_eq!(await_block["then"]["end"], 10);
        assert_eq!(await_block["catch"]["start"], 10);
    }

    #[test]
    fn concat_attribute_value_offsets_skip_spaces_after_equal() {
        let source = r#"<div class = "x{foo}y"></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);
        let value = &actual["fragment"]["nodes"][0]["attributes"][0]["value"];

        assert_eq!(value[0]["start"], 14);
        assert_eq!(value[1]["type"], "ExpressionTag");
        assert_eq!(value[1]["start"], 15);
        assert_eq!(value[1]["expression"]["start"], 16);
        assert_eq!(value[2]["start"], 20);
    }

    #[test]
    fn html_comment_markers_inside_earlier_script_are_not_script_leading_comments() {
        let source = r#"<script module>
const marker = "<!--not html-->";
</script>
<script>
let value;
</script>"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);

        assert!(actual["instance"]["content"]["leadingComments"].is_null());
    }

    #[test]
    fn script_leading_html_comment_does_not_cross_prior_script() {
        let source = r#"<!--module only-->
<script module>
export const value = 1;
</script>
<script>
let value = 2;
</script>"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);

        assert_eq!(
            actual["module"]["content"]["leadingComments"][0]["value"],
            "module only"
        );
        assert!(actual["instance"]["content"]["leadingComments"].is_null());
    }

    #[test]
    fn nested_html_comment_before_script_is_not_script_leading_comment() {
        let source = r#"<div><!--nested--></div>
<script>
let value;
</script>"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);

        assert!(actual["instance"]["content"]["leadingComments"].is_null());
    }

    #[test]
    fn debug_tag_serializes_identifier_locations() {
        let source = "{@debug value, other}";
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let actual = to_modern_json(&result.ast, source);
        let identifiers = &actual["fragment"]["nodes"][0]["identifiers"];

        assert_eq!(identifiers[0]["name"], "value");
        assert_eq!(identifiers[0]["start"], 8);
        assert_eq!(identifiers[0]["end"], 13);
        assert_eq!(identifiers[1]["name"], "other");
        assert_eq!(identifiers[1]["start"], 15);
        assert_eq!(identifiers[1]["end"], 20);
    }

    #[test]
    fn const_tag_serializes_estree_declaration() {
        let source = "{@const foo = bar}";
        let alloc = oxc::allocator::Allocator::default();
        let result = crate::parser::parse(source, &alloc);
        let modern = to_modern_json(&result.ast, source);
        let legacy = to_legacy_json(&result.ast, source);

        assert_eq!(
            modern["fragment"]["nodes"][0]["declaration"]["type"],
            "VariableDeclaration"
        );
        assert_eq!(
            modern["fragment"]["nodes"][0]["declaration"]["declarations"][0]["id"]["start"],
            8
        );
        assert_eq!(
            legacy["html"]["children"][0]["expression"]["type"],
            "AssignmentExpression"
        );
    }
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
/// Also handles comment attachment within the expression.
fn expression_to_estree(source: &str, expr_str: &str, expr_start: u32) -> Value {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let has_comments = expr_str.contains("//") || expr_str.contains("/*");

    // If expression contains comments and expr_start is large enough for the wrapper approach,
    // use Parser::parse() with a wrapper to get full comment information
    if has_comments && expr_start >= 7 {
        let wrapper = format!("void (\n{}\n)", expr_str);
        let alloc = Allocator::default();
        let parse_result = Parser::new(&alloc, &wrapper, SourceType::mjs()).parse();

        if !parse_result.program.body.is_empty() && parse_result.errors.is_empty() {
            if let Some(oxc::ast::ast::Statement::ExpressionStatement(es)) =
                parse_result.program.body.first()
            {
                if let oxc::ast::ast::Expression::UnaryExpression(unary) = &es.expression {
                    if let oxc::ast::ast::Expression::ParenthesizedExpression(paren) =
                        &unary.argument
                    {
                        let inner_expr = &paren.expression;
                        let wrapper_prefix_len = 7u32; // "void (\n"
                        let offset = expr_start - wrapper_prefix_len;
                        let mut result = estree_expr(inner_expr, source, offset);

                        let comments = &parse_result.program.comments;
                        if !comments.is_empty() {
                            attach_expression_comments(
                                &mut result,
                                comments,
                                expr_str,
                                expr_start,
                                wrapper_prefix_len,
                                source,
                            );
                        }

                        return result;
                    }
                }
            }
        }
    }

    // Standard path: use parse_expression directly
    let alloc = Allocator::default();
    let js_result = Parser::new(&alloc, expr_str, SourceType::mjs()).parse_expression();

    if let Ok(expr) = &js_result {
        if !expr_str.ends_with('.') {
            let mut result = estree_expr(expr, source, expr_start);
            if has_comments {
                add_leading_comment_from_text(&mut result, expr_str, expr_start);
            }
            return result;
        }
    }

    // Try TypeScript
    let alloc_ts = Allocator::default();
    let ts_result = Parser::new(&alloc_ts, expr_str, SourceType::ts()).parse_expression();

    match ts_result {
        Ok(expr) if !expr_str.ends_with('.') => {
            return estree_expr(&expr, source, expr_start);
        }
        _ => {}
    }

    // Final fallback: empty identifier for invalid expressions
    let expr_end = expr_start + expr_str.len() as u32;
    json!({
        "type": "Identifier",
        "start": expr_start,
        "end": expr_end,
        "name": ""
    })
}

/// Add leading comment from expression text (for the fallback path).
fn add_leading_comment_from_text(result: &mut Value, expr_str: &str, expr_start: u32) {
    let trimmed = expr_str.trim_start();
    if trimmed.starts_with("/*") {
        if let Some(end_pos) = trimmed.find("*/") {
            let comment_text = &trimmed[2..end_pos];
            let ws_offset = (expr_str.len() - trimmed.len()) as u32;
            let comment_start = expr_start + ws_offset;
            let comment_end = comment_start + end_pos as u32 + 2;
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "leadingComments".to_string(),
                    json!([{
                        "type": "Block",
                        "value": comment_text,
                        "start": comment_start,
                        "end": comment_end
                    }]),
                );
            }
        }
    } else if trimmed.starts_with("//") {
        if let Some(nl_pos) = trimmed.find('\n') {
            let comment_text = &trimmed[2..nl_pos];
            let ws_offset = (expr_str.len() - trimmed.len()) as u32;
            let comment_start = expr_start + ws_offset;
            let comment_end = comment_start + nl_pos as u32;
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "leadingComments".to_string(),
                    json!([{
                        "type": "Line",
                        "value": comment_text,
                        "start": comment_start,
                        "end": comment_end
                    }]),
                );
            }
        }
    }
}

/// Attach comments from a full parse result to an expression's estree JSON.
fn attach_expression_comments(
    result: &mut Value,
    comments: &[oxc::ast::ast::Comment],
    expr_str: &str,
    expr_start: u32,
    wrapper_prefix_len: u32,
    source: &str,
) {
    for c in comments.iter() {
        // Adjust comment positions from wrapper coords to source coords
        let c_start_in_expr = c.span.start.saturating_sub(wrapper_prefix_len);
        let c_end_in_expr = c.span.end.saturating_sub(wrapper_prefix_len);

        // Skip comments that are outside the expression range
        if c_start_in_expr > expr_str.len() as u32 {
            continue;
        }

        let c_start = expr_start + c_start_in_expr;
        let c_end = expr_start + c_end_in_expr;

        let comment_type = if c.is_line() { "Line" } else { "Block" };
        let raw = &expr_str
            [c_start_in_expr as usize..std::cmp::min(c_end_in_expr as usize, expr_str.len())];
        let value = if c.is_line() {
            raw.strip_prefix("//").unwrap_or(raw)
        } else {
            raw.strip_prefix("/*")
                .and_then(|v| v.strip_suffix("*/"))
                .unwrap_or(raw)
        };

        let comment_json = json!({
            "type": comment_type,
            "value": value,
            "start": c_start,
            "end": c_end
        });

        let attached_in_expr = c.attached_to.saturating_sub(wrapper_prefix_len);
        let attached_abs = expr_start + attached_in_expr;

        // Try to attach to the expression tree recursively
        if !attach_comment_recursive(result, &comment_json, attached_abs, c_start, source) {
            // Fallback: if the comment is after the expression (trailing), attach to the root
            let root_end = result.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if c_start >= root_end {
                if let Some(obj) = result.as_object_mut() {
                    let arr = obj.entry("trailingComments").or_insert(json!([]));
                    if let Some(a) = arr.as_array_mut() {
                        a.push(comment_json);
                    }
                }
            }
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
            let args: Vec<Value> = call
                .arguments
                .iter()
                .map(|a| {
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
                })
                .collect();
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
            let quasis: Vec<Value> = tl
                .quasis
                .iter()
                .map(|q| {
                    let q_start = offset + q.span.start;
                    let q_end = offset + q.span.end;
                    json!({
                        "type": "TemplateElement",
                        "start": q_start,
                        "end": q_end,
                        "loc": loc_json(source, q_start, q_end),
                        "value": {
                            "raw": q.value.raw.as_str(),
                            "cooked": q.value.cooked.as_ref().map(|c| c.as_str())
                        },
                        "tail": q.tail
                    })
                })
                .collect();
            let exprs: Vec<Value> = tl
                .expressions
                .iter()
                .map(|e| estree_expr(e, source, offset))
                .collect();
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
            let elements: Vec<Value> = arr
                .elements
                .iter()
                .map(|el| match el {
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
                    oxc::ast::ast::ArrayExpressionElement::Elision(_e) => Value::Null,
                    _ => estree_expr(el.as_expression().unwrap(), source, offset),
                })
                .collect();
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
            let properties: Vec<Value> = obj
                .properties
                .iter()
                .map(|prop| match prop {
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
                })
                .collect();
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
            let params: Vec<Value> = arrow
                .params
                .items
                .iter()
                .map(|p| estree_binding_pattern(p, source, offset))
                .collect();
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
                let body_start = offset + arrow.body.span.start;
                let body_end = offset + arrow.body.span.end;
                let stmts: Vec<Value> = arrow
                    .body
                    .statements
                    .iter()
                    .map(|s| serialize_statement_legacy(s, source, offset))
                    .collect();
                json!({
                    "type": "BlockStatement",
                    "start": body_start,
                    "end": body_end,
                    "loc": loc_json(source, body_start, body_end),
                    "body": stmts
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
            let exprs: Vec<Value> = seq
                .expressions
                .iter()
                .map(|e| estree_expr(e, source, offset))
                .collect();
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
            let quasis: Vec<Value> = tl
                .quasis
                .iter()
                .map(|q| {
                    let q_start = offset + q.span.start;
                    let q_end = offset + q.span.end;
                    json!({
                        "type": "TemplateElement",
                        "start": q_start,
                        "end": q_end,
                        "loc": loc_json(source, q_start, q_end),
                        "value": {
                            "raw": q.value.raw.as_str(),
                            "cooked": q.value.cooked.as_ref().map(|c| c.as_str())
                        },
                        "tail": q.tail
                    })
                })
                .collect();
            let exprs: Vec<Value> = tl
                .expressions
                .iter()
                .map(|e| estree_expr(e, source, offset))
                .collect();
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
        Expression::ImportExpression(imp) => {
            let start = offset + imp.span.start;
            let end = offset + imp.span.end;
            json!({
                "type": "ImportExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "source": estree_expr(&imp.source, source, offset),
                "options": Value::Null
            })
        }
        Expression::FunctionExpression(func) => {
            let start = offset + func.span.start;
            let end = offset + func.span.end;
            let params: Vec<Value> = func
                .params
                .items
                .iter()
                .map(|p| estree_binding_pattern(p, source, offset))
                .collect();
            json!({
                "type": "FunctionExpression",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "id": func.id.as_ref().map(|id| {
                    json!({
                        "type": "Identifier",
                        "start": offset + id.span.start,
                        "end": offset + id.span.end,
                        "name": id.name.as_str()
                    })
                }),
                "generator": func.generator,
                "async": func.r#async,
                "params": params,
                "body": {
                    "type": "BlockStatement",
                    "start": offset + func.body.as_ref().map(|b| b.span.start).unwrap_or(0),
                    "end": offset + func.body.as_ref().map(|b| b.span.end).unwrap_or(0),
                    "body": func.body.as_ref().map(|b| {
                        b.statements.iter().map(|s| serialize_statement_legacy(s, source, offset)).collect::<Vec<_>>()
                    }).unwrap_or_default()
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

fn estree_expr_from_callee(
    callee: &oxc::ast::ast::Expression<'_>,
    source: &str,
    offset: u32,
) -> Value {
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
        _ => estree_expr(key.as_expression().unwrap(), source, offset),
    }
}

fn estree_binding_pattern(
    pattern: &oxc::ast::ast::FormalParameter<'_>,
    source: &str,
    offset: u32,
) -> Value {
    let mut result = estree_binding_pat(&pattern.pattern, source, offset);
    // If the FormalParameter has a type annotation, use the FormalParameter's span
    // and add typeAnnotation field
    if let Some(type_ann) = &pattern.type_annotation {
        let _param_start = offset + pattern.span.start;
        let param_end = offset + pattern.span.end;
        // Update the end to include the type annotation
        if let Some(obj) = result.as_object_mut() {
            obj.insert("end".to_string(), json!(param_end));
            if let Some(loc) = obj.get_mut("loc") {
                if let Some(loc_obj) = loc.as_object_mut() {
                    let (end_line, end_col) = offset_to_loc(source, param_end as usize);
                    loc_obj.insert(
                        "end".to_string(),
                        json!({"line": end_line, "column": end_col}),
                    );
                }
            }
            // Add typeAnnotation with nested type
            let ann_start = offset + type_ann.span.start;
            let ann_end = offset + type_ann.span.end;
            // Get the type keyword from the annotation
            let type_node = serialize_ts_type(&type_ann.type_annotation, source, offset);
            obj.insert(
                "typeAnnotation".to_string(),
                json!({
                    "type": "TSTypeAnnotation",
                    "start": ann_start,
                    "end": ann_end,
                    "loc": loc_json(source, ann_start, ann_end),
                    "typeAnnotation": type_node
                }),
            );
        }
    }
    result
}

/// Recursively try to attach a comment to a node whose start matches attached_to.
fn attach_comment_recursive(
    node: &mut Value,
    comment: &Value,
    attached_to: u32,
    comment_start: u32,
    source: &str,
) -> bool {
    if let Some(obj) = node.as_object_mut() {
        let node_start = obj
            .get("start")
            .and_then(|v| v.as_u64())
            .unwrap_or(u64::MAX) as u32;
        let node_end = obj.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

        // Check if this node is the leading target (attached_to matches node start)
        if node_start == attached_to {
            let node_end_line = offset_to_loc(source, node_end as usize).0;
            let comment_line = offset_to_loc(source, comment_start as usize).0;
            let field = if comment_line == node_end_line && comment_start >= node_end {
                "trailingComments"
            } else {
                "leadingComments"
            };
            let arr = obj.entry(field).or_insert(json!([]));
            if let Some(a) = arr.as_array_mut() {
                a.push(comment.clone());
            }
            return true;
        }

        // If comment position is within this node's range, recurse into children
        if comment_start >= node_start && comment_start <= node_end {
            // First, try to find exact match in children
            for (key, v) in obj.iter_mut() {
                // Skip comment-related fields
                if key == "leadingComments" || key == "trailingComments" {
                    continue;
                }
                match v {
                    Value::Object(_) => {
                        if attach_comment_recursive(v, comment, attached_to, comment_start, source)
                        {
                            return true;
                        }
                    }
                    Value::Array(arr) => {
                        // For array children (like body, elements, properties),
                        // also check if the comment trails a specific element
                        for item in arr.iter_mut() {
                            if attach_comment_recursive(
                                item,
                                comment,
                                attached_to,
                                comment_start,
                                source,
                            ) {
                                return true;
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Try trailing attachment: find the last array element that ends before the comment.
            // This handles cases like:
            //   [1, // trailing comment 1
            //    /* trailing comment 2 */]
            // where comment 2 is on a different line but still trails element 1.
            for (key, v) in obj.iter_mut() {
                if key == "leadingComments" || key == "trailingComments" {
                    continue;
                }
                if let Value::Array(arr) = v {
                    // Find the last element whose end is before the comment
                    let mut last_before_idx: Option<usize> = None;
                    for (i, item) in arr.iter().enumerate() {
                        let item_end = item.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if item_end <= comment_start {
                            last_before_idx = Some(i);
                        }
                    }
                    if let Some(idx) = last_before_idx {
                        // Check that no other element starts between this element's end and the comment
                        let item_end =
                            arr[idx].get("end").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let no_other_after = arr.iter().skip(idx + 1).all(|item| {
                            let s = item.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            s > comment_start
                        });
                        if no_other_after && item_end <= comment_start {
                            if let Some(item_obj) = arr[idx].as_object_mut() {
                                let entry = item_obj.entry("trailingComments").or_insert(json!([]));
                                if let Some(a) = entry.as_array_mut() {
                                    a.push(comment.clone());
                                }
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Strip internal fields (starting with _) from JSON values recursively.
fn strip_internal_fields(values: &mut Vec<Value>) {
    for v in values.iter_mut() {
        strip_internal_fields_value(v);
    }
}

fn strip_internal_fields_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|k, _| !k.starts_with('_'));
            for (_, v) in map.iter_mut() {
                strip_internal_fields_value(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_internal_fields_value(v);
            }
        }
        _ => {}
    }
}

/// Adjust all column values +1 in loc objects for multi-line destructured patterns.
/// The Svelte compiler uses acorn which has a different column convention for patterns.
fn adjust_binding_columns(value: &mut Value, source: &str) {
    match value {
        Value::Object(map) => {
            // Check if this is a loc-like object with line/column
            if map.contains_key("line") && map.contains_key("column") {
                if let Some(line) = map.get("line").and_then(|v| v.as_u64()) {
                    if line > 1 {
                        if let Some(col) = map.get("column").and_then(|v| v.as_u64()) {
                            map.insert("column".to_string(), json!(col + 1));
                        }
                    }
                }
            }
            for (_, v) in map.iter_mut() {
                adjust_binding_columns(v, source);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                adjust_binding_columns(v, source);
            }
        }
        _ => {}
    }
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
            let properties: Vec<Value> = obj
                .properties
                .iter()
                .map(|p| {
                    let p_start = offset + p.span.start;
                    let p_end = offset + p.span.end;
                    let key = estree_property_key(&p.key, source, offset);
                    let value = estree_binding_pat(&p.value, source, offset);
                    json!({
                        "type": "Property",
                        "start": p_start,
                        "end": p_end,
                        "loc": loc_json(source, p_start, p_end),
                        "method": false,
                        "shorthand": p.shorthand,
                        "computed": p.computed,
                        "key": key,
                        "value": value,
                        "kind": "init"
                    })
                })
                .collect();
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
            let mut elements: Vec<Value> = arr
                .elements
                .iter()
                .map(|el| match el {
                    Some(pat) => estree_binding_pat(pat, source, offset),
                    None => Value::Null,
                })
                .collect();
            // Include rest element (...rest)
            if let Some(rest) = &arr.rest {
                let r_start = offset + rest.span.start;
                let r_end = offset + rest.span.end;
                elements.push(json!({
                    "type": "RestElement",
                    "start": r_start,
                    "end": r_end,
                    "loc": loc_json(source, r_start, r_end),
                    "argument": estree_binding_pat(&rest.argument, source, offset)
                }));
            }
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

fn serialize_ts_type(ts_type: &oxc::ast::ast::TSType<'_>, source: &str, offset: u32) -> Value {
    use oxc::ast::ast::TSType;
    let span = match ts_type {
        TSType::TSStringKeyword(t) => t.span,
        TSType::TSNumberKeyword(t) => t.span,
        TSType::TSBooleanKeyword(t) => t.span,
        TSType::TSAnyKeyword(t) => t.span,
        TSType::TSVoidKeyword(t) => t.span,
        TSType::TSNullKeyword(t) => t.span,
        TSType::TSUndefinedKeyword(t) => t.span,
        TSType::TSTypeReference(t) => {
            let start = offset + t.span.start;
            let end = offset + t.span.end;
            let type_name_node = match &t.type_name {
                oxc::ast::ast::TSTypeName::IdentifierReference(id) => {
                    let id_start = offset + id.span.start;
                    let id_end = offset + id.span.end;
                    json!({
                        "type": "Identifier",
                        "start": id_start,
                        "end": id_end,
                        "loc": loc_json(source, id_start, id_end),
                        "name": id.name.as_str()
                    })
                }
                oxc::ast::ast::TSTypeName::QualifiedName(q) => {
                    json!({ "type": "TSQualifiedName", "start": offset + q.span.start, "end": offset + q.span.end })
                }
                _ => {
                    json!({ "type": "Identifier", "name": "this" })
                }
            };
            return json!({
                "type": "TSTypeReference",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "typeName": type_name_node
            });
        }
        _ => return json!({ "type": "TSUnknownType" }),
    };
    let start = offset + span.start;
    let end = offset + span.end;
    let type_name = match ts_type {
        TSType::TSStringKeyword(_) => "TSStringKeyword",
        TSType::TSNumberKeyword(_) => "TSNumberKeyword",
        TSType::TSBooleanKeyword(_) => "TSBooleanKeyword",
        TSType::TSAnyKeyword(_) => "TSAnyKeyword",
        TSType::TSVoidKeyword(_) => "TSVoidKeyword",
        TSType::TSNullKeyword(_) => "TSNullKeyword",
        TSType::TSUndefinedKeyword(_) => "TSUndefinedKeyword",
        _ => "TSUnknownType",
    };
    json!({
        "type": type_name,
        "start": start,
        "end": end,
        "loc": loc_json(source, start, end)
    })
}

fn estree_assignment_target(
    target: &oxc::ast::ast::AssignmentTarget<'_>,
    source: &str,
    offset: u32,
) -> Value {
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
        _ => json!({ "type": "UnknownTarget" }),
    }
}

fn estree_simple_assign_target(
    target: &oxc::ast::ast::SimpleAssignmentTarget<'_>,
    source: &str,
    offset: u32,
) -> Value {
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
        _ => json!({ "type": "UnknownTarget" }),
    }
}

/// Filter out whitespace-only text nodes from block content.
fn filter_whitespace_nodes<'a, 'b>(nodes: &'b [TemplateNode<'a>]) -> Vec<&'b TemplateNode<'a>> {
    nodes
        .iter()
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
fn strip_trailing_whitespace<'a, 'b>(nodes: &'b [TemplateNode<'a>]) -> Vec<&'b TemplateNode<'a>> {
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

fn serialize_filtered_children_ctx(
    nodes: &[TemplateNode],
    source: &str,
    default_end: u32,
    in_svelte_head: bool,
) -> (Vec<Value>, u32) {
    let filtered = filter_whitespace_nodes(nodes);
    let children: Vec<Value> = filtered
        .iter()
        .map(|n| serialize_node_legacy_ctx(n, source, in_svelte_head))
        .collect();
    let end = filtered
        .last()
        .map(|n| node_span_end(n))
        .unwrap_or(default_end);
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
                    "amp" | "lt" | "gt" | "quot" | "apos" | "nbsp" => match entity_name {
                        "amp" => result.push('&'),
                        "lt" => result.push('<'),
                        "gt" => result.push('>'),
                        "quot" => result.push('"'),
                        "apos" => result.push('\''),
                        "nbsp" => result.push('\u{00A0}'),
                        _ => unreachable!(),
                    },
                    _ => {
                        // Check if entity starts with a known name followed by non-alnum
                        let mut decoded = false;
                        for known in &["amp", "lt", "gt", "quot", "apos", "nbsp"] {
                            if entity_name.starts_with(known) {
                                let rest = &entity_name[known.len()..];
                                if !rest.is_empty()
                                    && !rest.starts_with(|c: char| c.is_alphanumeric())
                                {
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

fn serialize_attribute_modern(attr: &Attribute, meta: &AttributeMeta, source: &str) -> Value {
    match attr {
        Attribute::NormalAttribute { name, value, span } => {
            if name == "@attach" {
                // AttachTag
                if let AttributeValue::Expression(expr) = value {
                    let expr_start = meta
                        .expression_span
                        .map(|s| trimmed_span_start(s, expr))
                        .unwrap_or(span.start);
                    return json!({
                        "type": "AttachTag",
                        "start": span.start,
                        "end": span.end,
                        "expression": expression_to_estree(source, expr.trim(), expr_start)
                    });
                }
            }
            // Regular attribute
            let (n_start, n_end) = if name.is_empty() {
                (meta.name_span.start, meta.name_span.end)
            } else {
                (meta.name_span.start, meta.name_span.end)
            };
            // Modern attribute value: Expression → ExpressionTag, not array
            let value_json = match value {
                AttributeValue::Expression(expr) => {
                    let trimmed = expr.trim();
                    let expression_span = meta.expression_span.unwrap_or(*span);
                    let mustache_span = meta.mustache_span.unwrap_or(*span);
                    let expr_start = trimmed_span_start(expression_span, expr);
                    if trimmed.is_empty() && name.is_empty() {
                        // Empty shorthand {} attribute: position inside braces
                        let inner_pos = expression_span.start;
                        let mut expr = expression_to_estree(source, "", inner_pos);
                        // Modern format: add loc to empty expression
                        if let Some(obj) = expr.as_object_mut() {
                            if !obj.contains_key("loc") {
                                obj.insert(
                                    "loc".to_string(),
                                    loc_json_with_char(source, inner_pos, inner_pos),
                                );
                            }
                        }
                        json!({
                            "type": "ExpressionTag",
                            "start": inner_pos,
                            "end": inner_pos,
                            "expression": expr
                        })
                    } else {
                        json!({
                            "type": "ExpressionTag",
                            "start": mustache_span.start,
                            "end": mustache_span.end,
                            "expression": expression_to_estree(source, trimmed, expr_start)
                        })
                    }
                }
                AttributeValue::True => json!(true),
                _ => serialize_attr_value_modern(value, meta, source),
            };
            json!({
                "type": "Attribute",
                "start": span.start,
                "end": span.end,
                "name": name,
                "name_loc": loc_json_with_char(source, n_start, n_end),
                "value": value_json
            })
        }
        Attribute::Spread { span } => {
            let expression_span = meta.expression_span.unwrap_or(*span);
            let expr_str = source_for_span(source, expression_span);
            let expr_start = trimmed_span_start(expression_span, expr_str);
            json!({
                "type": "SpreadAttribute",
                "start": span.start,
                "end": span.end,
                "expression": expression_to_estree(source, expr_str.trim(), expr_start)
            })
        }
        Attribute::Directive {
            kind,
            name,
            modifiers,
            value,
            span,
        } => {
            let type_name = match kind {
                DirectiveKind::EventHandler => "OnDirective",
                DirectiveKind::Binding => "BindDirective",
                DirectiveKind::Class => "ClassDirective",
                DirectiveKind::StyleDirective => "StyleDirective",
                DirectiveKind::Use => "UseDirective",
                DirectiveKind::Transition => "TransitionDirective",
                DirectiveKind::In => "TransitionDirective",
                DirectiveKind::Out => "TransitionDirective",
                DirectiveKind::Animate => "AnimateDirective",
                DirectiveKind::Let => "LetDirective",
            };

            // Parse expression from directive value
            let expression = directive_expression_modern(value, meta, source);

            // Calculate name_loc for directive
            let mut obj = json!({
                "type": type_name,
                "start": span.start,
                "end": span.end,
                "name": name,
                "name_loc": loc_json_with_char(source, meta.name_span.start, meta.name_span.end),
                "modifiers": modifiers
            });

            if matches!(kind, DirectiveKind::StyleDirective) {
                obj["value"] = style_directive_value_modern(value, meta, source);
            } else if let Some(expr) = expression {
                obj["expression"] = expr;
            } else {
                obj["expression"] = Value::Null;
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

fn directive_expression_modern(
    value: &AttributeValue,
    meta: &AttributeMeta,
    source: &str,
) -> Option<Value> {
    match value {
        AttributeValue::Expression(expr) => {
            let expression_span = meta.expression_span?;
            Some(expression_to_estree(
                source,
                expr.trim(),
                trimmed_span_start(expression_span, expr),
            ))
        }
        AttributeValue::Concat(parts) if parts.len() == 1 => {
            let AttributeValuePart::Expression(expr) = &parts[0] else {
                return None;
            };
            let part_meta = meta.parts.first()?;
            let expression_span = part_meta.expression_span?;
            Some(expression_to_estree(
                source,
                expr.trim(),
                trimmed_span_start(expression_span, expr),
            ))
        }
        _ => None,
    }
}

fn serialize_attr_value_modern(
    value: &AttributeValue,
    meta: &AttributeMeta,
    source: &str,
) -> Value {
    match value {
        AttributeValue::True => json!(true),
        AttributeValue::Static(s) => {
            let value_span = meta.value_span.unwrap_or(meta.name_span);
            json!([{
                "start": value_span.start,
                "end": value_span.end,
                "type": "Text",
                "raw": s,
                "data": decode_entities(s)
            }])
        }
        AttributeValue::Expression(expr) => {
            let expression_span = meta.expression_span.unwrap_or(meta.name_span);
            let mustache_span = meta.mustache_span.unwrap_or(expression_span);
            json!({
                "type": "ExpressionTag",
                "start": mustache_span.start,
                "end": mustache_span.end,
                "expression": expression_to_estree(
                    source,
                    expr.trim(),
                    trimmed_span_start(expression_span, expr),
                )
            })
        }
        AttributeValue::Concat(parts) => {
            let values: Vec<Value> = parts
                .iter()
                .zip(meta.parts.iter())
                .map(|(part, part_meta)| match part {
                    AttributeValuePart::Static(s) => json!({
                        "start": part_meta.span.start,
                        "end": part_meta.span.end,
                        "type": "Text",
                        "raw": s,
                        "data": decode_entities(s)
                    }),
                    AttributeValuePart::Expression(expr) => {
                        let expression_span = part_meta.expression_span.unwrap_or(part_meta.span);
                        let mustache_span = part_meta.mustache_span.unwrap_or(part_meta.span);
                        json!({
                            "type": "ExpressionTag",
                            "start": mustache_span.start,
                            "end": mustache_span.end,
                            "expression": expression_to_estree(
                                source,
                                expr.trim(),
                                trimmed_span_start(expression_span, expr),
                            )
                        })
                    }
                })
                .collect();
            Value::Array(values)
        }
    }
}

fn style_directive_value_modern(
    value: &AttributeValue,
    meta: &AttributeMeta,
    source: &str,
) -> Value {
    match value {
        AttributeValue::True => json!(true),
        AttributeValue::Expression(expr) => {
            let expression_span = meta.expression_span.unwrap_or(meta.name_span);
            let mustache_span = meta.mustache_span.unwrap_or(expression_span);
            json!({
                "type": "ExpressionTag",
                "start": mustache_span.start,
                "end": mustache_span.end,
                "expression": expression_to_estree(
                    source,
                    expr.trim(),
                    trimmed_span_start(expression_span, expr),
                )
            })
        }
        AttributeValue::Concat(parts) if parts.len() == 1 => {
            if let AttributeValuePart::Expression(expr) = &parts[0] {
                let part_meta = meta.parts.first();
                let expression_span = part_meta
                    .and_then(|meta| meta.expression_span)
                    .unwrap_or(meta.name_span);
                let mustache_span = part_meta
                    .and_then(|meta| meta.mustache_span)
                    .unwrap_or(expression_span);
                json!({
                    "type": "ExpressionTag",
                    "start": mustache_span.start,
                    "end": mustache_span.end,
                    "expression": expression_to_estree(
                        source,
                        expr.trim(),
                        trimmed_span_start(expression_span, expr),
                    )
                })
            } else {
                serialize_attr_value_modern(value, meta, source)
            }
        }
        AttributeValue::Static(_) | AttributeValue::Concat(_) => {
            serialize_attr_value_modern(value, meta, source)
        }
    }
}

fn directive_expression_legacy(
    value: &AttributeValue,
    meta: &AttributeMeta,
    source: &str,
) -> Option<Value> {
    match value {
        AttributeValue::True => None,
        AttributeValue::Expression(expr) => {
            let expression_span = meta.expression_span?;
            Some(expression_to_estree(
                source,
                expr.trim(),
                trimmed_span_start(expression_span, expr),
            ))
        }
        AttributeValue::Concat(parts) if parts.len() == 1 => {
            let AttributeValuePart::Expression(expr) = &parts[0] else {
                return Some(serialize_attr_value_legacy(value, meta, source));
            };
            let part_meta = meta.parts.first()?;
            let expression_span = part_meta.expression_span?;
            Some(expression_to_estree(
                source,
                expr.trim(),
                trimmed_span_start(expression_span, expr),
            ))
        }
        AttributeValue::Static(_) | AttributeValue::Concat(_) => {
            Some(serialize_attr_value_legacy(value, meta, source))
        }
    }
}

fn serialize_raw_tag_attributes(source: &str, attrs_span: oxc::span::Span) -> Vec<Value> {
    let start = attrs_span.start as usize;
    let end = attrs_span.end as usize;
    if start >= end || end > source.len() {
        return Vec::new();
    }

    let bytes = source.as_bytes();
    let mut pos = start;
    let mut attributes = Vec::new();

    while pos < end {
        while pos < end && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b'/') {
            pos += 1;
        }

        if pos + 1 < end && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            pos += 2;
            while pos < end && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }

        if pos + 1 < end && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos += 2;
            while pos + 1 < end && !(bytes[pos] == b'*' && bytes[pos + 1] == b'/') {
                pos += 1;
            }
            pos = (pos + 2).min(end);
            continue;
        }

        if pos >= end {
            break;
        }

        let attr_start = pos;
        while pos < end {
            let ch = bytes[pos];
            if ch.is_ascii_whitespace() || matches!(ch, b'=' | b'/' | b'>') {
                break;
            }
            pos += 1;
        }

        if attr_start == pos {
            pos += source[pos..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
            continue;
        }

        let name_end = pos;
        let name = &source[attr_start..name_end];
        let mut attr_end = name_end;

        while pos < end && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let value = if pos < end && bytes[pos] == b'=' {
            pos += 1;
            while pos < end && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }

            if pos >= end {
                json!([])
            } else if matches!(bytes[pos], b'"' | b'\'') {
                let quote = bytes[pos];
                pos += 1;
                let value_start = pos;
                while pos < end && bytes[pos] != quote {
                    if bytes[pos] == b'\\' {
                        pos = (pos + 2).min(end);
                    } else {
                        pos += source[pos..]
                            .chars()
                            .next()
                            .map(char::len_utf8)
                            .unwrap_or(1);
                    }
                }
                let value_end = pos;
                if pos < end {
                    pos += 1;
                }
                let raw = &source[value_start..value_end];
                attr_end = pos;
                json!([{
                    "start": value_start as u32,
                    "end": value_end as u32,
                    "type": "Text",
                    "raw": raw,
                    "data": decode_entities(raw)
                }])
            } else {
                let value_start = pos;
                while pos < end {
                    let ch = bytes[pos];
                    if ch.is_ascii_whitespace() || matches!(ch, b'/' | b'>') {
                        break;
                    }
                    pos += source[pos..]
                        .chars()
                        .next()
                        .map(char::len_utf8)
                        .unwrap_or(1);
                }
                let raw = &source[value_start..pos];
                attr_end = pos;
                json!([{
                    "start": value_start as u32,
                    "end": pos as u32,
                    "type": "Text",
                    "raw": raw,
                    "data": decode_entities(raw)
                }])
            }
        } else {
            json!(true)
        };

        attributes.push(json!({
            "type": "Attribute",
            "start": attr_start as u32,
            "end": attr_end as u32,
            "name": name,
            "name_loc": loc_json_with_char(source, attr_start as u32, name_end as u32),
            "value": value
        }));
    }

    attributes
}

/// Convert legacy CSS AST to modern format (Selector → ComplexSelector/RelativeSelector).
fn convert_css_to_modern(children: &[Value]) -> Vec<Value> {
    children
        .iter()
        .map(|child| {
            let mut c = child.clone();
            if let Some(obj) = c.as_object_mut() {
                // Convert SelectorList children from Selector to ComplexSelector
                if let Some(prelude) = obj.get_mut("prelude") {
                    convert_selector_list_modern(prelude);
                }
                // Convert block children recursively
                if let Some(block) = obj.get_mut("block") {
                    if let Some(block_children) = block.get_mut("children") {
                        if let Some(arr) = block_children.as_array_mut() {
                            let converted = convert_css_to_modern(&arr.clone());
                            *block_children = json!(converted);
                        }
                    }
                }
            }
            c
        })
        .collect()
}

fn convert_selector_list_modern(selector_list: &mut Value) {
    if let Some(obj) = selector_list.as_object_mut() {
        if obj.get("type").and_then(|t| t.as_str()) == Some("SelectorList") {
            // Update end from _full_end if available
            if let Some(full_end) = obj.get("_full_end").cloned() {
                obj.insert("end".to_string(), full_end);
            }
            obj.remove("_full_end");
            // Convert children and update end
            let new_end = if let Some(children) = obj.get("children").and_then(|c| c.as_array()) {
                let converted: Vec<Value> = children
                    .iter()
                    .map(|selector| convert_selector_to_complex(selector))
                    .collect();
                let last_end = converted.last().and_then(|l| l.get("end")).cloned();
                obj.insert("children".to_string(), json!(converted));
                last_end
            } else {
                None
            };
            if let Some(end) = new_end {
                obj.insert("end".to_string(), end);
            }
        }
    }
}

fn convert_selector_to_complex(selector: &Value) -> Value {
    if let Some(obj) = selector.as_object() {
        if obj.get("type").and_then(|t| t.as_str()) == Some("Selector") {
            let start = obj.get("start").cloned().unwrap_or(json!(0));
            // Use _full_end for modern format (includes pseudo-element parens)
            let end = obj
                .get("_full_end")
                .cloned()
                .unwrap_or_else(|| obj.get("end").cloned().unwrap_or(json!(0)));
            let children = obj
                .get("children")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();

            // Group children into RelativeSelectors (split on Combinator)
            let mut relative_selectors = Vec::new();
            let mut current_selectors = Vec::new();
            let mut current_combinator = Value::Null;
            let mut rel_start = start.clone();

            for child in &children {
                let child_type = child.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if child_type == "Combinator" {
                    if !current_selectors.is_empty() {
                        let rel_end = current_selectors
                            .last()
                            .and_then(|s: &Value| s.get("end"))
                            .cloned()
                            .unwrap_or(json!(0));
                        relative_selectors.push(json!({
                            "type": "RelativeSelector",
                            "combinator": current_combinator,
                            "start": rel_start,
                            "end": rel_end,
                            "selectors": current_selectors
                        }));
                        current_selectors = Vec::new();
                    }
                    current_combinator = child.clone();
                    rel_start = child.get("start").cloned().unwrap_or(json!(0));
                } else {
                    // Recursively convert args in PseudoClassSelectors
                    let mut c = child.clone();
                    if let Some(obj) = c.as_object_mut() {
                        if let Some(args) = obj.get_mut("args") {
                            convert_selector_list_modern(args);
                        }
                    }
                    current_selectors.push(c);
                }
            }
            if !current_selectors.is_empty() {
                // Use the full selector end (includes pseudo-element parens)
                let rel_end = end.clone();
                relative_selectors.push(json!({
                    "type": "RelativeSelector",
                    "combinator": current_combinator,
                    "start": rel_start,
                    "end": rel_end,
                    "selectors": current_selectors
                }));
            }

            return json!({
                "type": "ComplexSelector",
                "start": start,
                "end": end,
                "children": relative_selectors
            });
        }
    }
    selector.clone()
}

/// Convert byte offsets to character offsets in a JSON value tree.
fn convert_byte_to_char_offsets(value: &mut Value, source: &str) {
    match value {
        Value::Object(map) => {
            for key in &["start", "end", "character"] {
                if let Some(v) = map.get_mut(*key) {
                    if let Some(n) = v.as_u64() {
                        *v = json!(byte_to_char_offset(source, n as usize));
                    }
                }
            }
            for (_, v) in map.iter_mut() {
                convert_byte_to_char_offsets(v, source);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                convert_byte_to_char_offsets(v, source);
            }
        }
        _ => {}
    }
}

/// Serialize a `SvelteAst` to the legacy Svelte JSON format.
/// Serialize a `SvelteAst` to the modern Svelte 5 JSON format.
pub fn to_modern_json(ast: &SvelteAst, source: &str) -> Value {
    let mut fragment = serialize_fragment_modern(&ast.html, source);

    // Extract <svelte:options> from fragment and put in root.options
    let mut options_val = Value::Null;
    if let Some(nodes) = fragment.get_mut("nodes") {
        if let Some(arr) = nodes.as_array_mut() {
            if let Some(idx) = arr
                .iter()
                .position(|n| n.get("type").and_then(|t| t.as_str()) == Some("SvelteOptionsRaw"))
            {
                let options_node = arr.remove(idx);
                let attrs = options_node.get("attributes").cloned().unwrap_or(json!([]));
                let mut opts = json!({
                    "start": options_node.get("start"),
                    "end": options_node.get("end"),
                    "attributes": attrs.clone()
                });
                // Extract specific option values from attributes
                if let Some(attr_arr) = attrs.as_array() {
                    for attr in attr_arr {
                        let name = attr.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        match name {
                            "customElement" => {
                                if let Some(val) = attr.get("value") {
                                    if let Some(arr) = val.as_array() {
                                        if let Some(text) = arr.first() {
                                            if let Some(data) =
                                                text.get("data").and_then(|d| d.as_str())
                                            {
                                                opts["customElement"] = json!({ "tag": data });
                                            }
                                        }
                                    }
                                }
                            }
                            "runes" => {
                                // runes={true} → runes: true
                                opts["runes"] = json!(true);
                            }
                            _ => {}
                        }
                    }
                }
                options_val = opts;
            }
        }
    }

    let end = source.len() as u32;

    // Add CSS as StyleSheet in modern format
    let css_val = if let Some(style) = &ast.css {
        let content_start = style.content_span.start;
        let content_end = style.content_span.end;
        let legacy_children = crate::parser::css::parse_css_children(&style.content, content_start);
        let children = convert_css_to_modern(&legacy_children);
        json!({
            "type": "StyleSheet",
            "start": style.span.start,
            "end": style.span.end,
            "attributes": serialize_raw_tag_attributes(source, style.attrs_span),
            "children": children,
            "content": {
                "start": content_start,
                "end": content_end,
                "styles": style.content,
                "comment": null
            }
        })
    } else {
        Value::Null
    };

    // Add instance in modern format (with attributes from script tag)
    let instance_val = if let Some(script) = &ast.instance {
        let leading_comments = html_comments_before(&ast.html.nodes, script.span.start);
        let mut s = serialize_script_legacy(script, source, "default", &leading_comments);
        s["attributes"] = json!(serialize_raw_tag_attributes(source, script.attrs_span));
        s
    } else {
        Value::Null
    };

    let mut root = json!({
        "css": css_val,
        "js": [],
        "start": 0,
        "end": end,
        "type": "Root",
        "fragment": fragment,
        "options": options_val
    });

    if ast.instance.is_some() {
        root["instance"] = instance_val;
    }

    // Add module script in modern format
    if let Some(module) = &ast.module {
        let leading_comments = html_comments_before(&ast.html.nodes, module.span.start);
        let mut m = serialize_script_legacy(module, source, "module", &leading_comments);
        m["attributes"] = json!(serialize_raw_tag_attributes(source, module.attrs_span));
        root["module"] = m;
    }

    // Collect JS comments from scripts and template expressions for modern format.
    let mut js_comments = Vec::new();
    for script in [&ast.instance, &ast.module].into_iter().flatten() {
        let content_start = script.content_span.start;

        use oxc::allocator::Allocator;
        use oxc::parser::Parser;
        use oxc::span::SourceType;
        let alloc = Allocator::default();
        let source_type = if script.lang.as_deref() == Some("ts") {
            SourceType::ts()
        } else {
            SourceType::mjs()
        };
        let result = Parser::new(&alloc, &script.content, source_type).parse();
        for comment in result.program.comments.iter() {
            let start = content_start + comment.span.start;
            let end = content_start + comment.span.end;
            let raw = &script.content[comment.span.start as usize..comment.span.end as usize];
            let value = if comment.is_line() {
                raw.strip_prefix("//").unwrap_or(raw)
            } else {
                raw.strip_prefix("/*")
                    .and_then(|value| value.strip_suffix("*/"))
                    .unwrap_or(raw)
            };
            js_comments.push(json!({
                "type": if comment.is_line() { "Line" } else { "Block" },
                "value": value,
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end)
            }));
        }
    }

    // Exclude comments inside <script> and <style> blocks while scanning template text.
    let script_ranges: Vec<(usize, usize)> = [&ast.instance, &ast.module]
        .iter()
        .filter_map(|s| s.as_ref())
        .map(|s| (s.span.start as usize, s.span.end as usize))
        .chain(
            ast.css
                .as_ref()
                .map(|s| (s.span.start as usize, s.span.end as usize)),
        )
        .collect();
    let in_script_or_style =
        |pos: usize| -> bool { script_ranges.iter().any(|(s, e)| pos >= *s && pos < *e) };
    {
        let bytes = source.as_bytes();
        let mut i = 0;
        let mut brace_depth = 0i32; // track { } nesting to skip comments inside expressions
        while i < source.len() {
            if in_script_or_style(i) {
                i += 1;
                continue;
            }
            // Track brace depth for skipping comments inside {expressions}
            if bytes[i] == b'{' {
                brace_depth += 1;
                i += 1;
                continue;
            }
            if bytes[i] == b'}' {
                brace_depth -= 1;
                i += 1;
                continue;
            }
            if bytes[i] == b'"' || bytes[i] == b'\'' || bytes[i] == b'`' {
                let q = bytes[i];
                i += 1;
                while i < source.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < source.len() {
                    i += 1;
                }
            } else if i + 1 < source.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                let start = i as u32;
                i += 2;
                let value_start = i;
                while i < source.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                let loc = if brace_depth > 0 {
                    loc_json(source, start, i as u32)
                } else {
                    loc_json_with_char(source, start, i as u32)
                };
                js_comments.push(json!({
                    "type": "Line", "start": start, "end": i as u32,
                    "value": &source[value_start..i],
                    "loc": loc
                }));
            } else if i + 1 < source.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                let start = i as u32;
                i += 2;
                let value_start = i;
                while i + 1 < source.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                let value = &source[value_start..i];
                i += 2;
                let loc = if brace_depth > 0 {
                    loc_json(source, start, i as u32)
                } else {
                    loc_json_with_char(source, start, i as u32)
                };
                js_comments.push(json!({
                    "type": "Block", "start": start, "end": i as u32,
                    "value": value,
                    "loc": loc
                }));
            } else {
                i += 1;
            }
        }
    }
    js_comments.sort_by_key(|comment| {
        comment
            .get("start")
            .and_then(|start| start.as_u64())
            .unwrap_or_default()
    });
    root["comments"] = json!(js_comments);

    if has_multibyte(source) {
        convert_byte_to_char_offsets(&mut root, source);
    }

    root
}

fn serialize_fragment_modern(fragment: &Fragment, source: &str) -> Value {
    serialize_fragment_modern_ctx(fragment, source, false, false)
}

fn serialize_fragment_modern_ctx(
    fragment: &Fragment,
    source: &str,
    in_shadow_root: bool,
    in_svelte_head: bool,
) -> Value {
    let nodes: Vec<Value> = fragment
        .nodes
        .iter()
        .filter(|n| {
            if let TemplateNode::Text(t) = n {
                if t.data.chars().all(|c| c.is_ascii_whitespace())
                    && t.span.end as usize >= source.len() - 1
                {
                    return false;
                }
            }
            true
        })
        .map(|n| serialize_node_modern_ctx(n, source, in_shadow_root, in_svelte_head))
        .collect();
    json!({
        "type": "Fragment",
        "nodes": nodes
    })
}

#[allow(dead_code)]
fn serialize_node_modern(node: &TemplateNode, source: &str) -> Value {
    serialize_node_modern_ctx(node, source, false, false)
}

/// Translate a span end to the JSON value the modern + legacy serializers
/// emit. Mirrors vendor Svelte's `-1` sentinel for AST nodes left on the
/// parser's stack at EOF when they aren't the topmost open node — vendor
/// only adjusts the topmost to `template.length`, leaving every other
/// in-stack node's `end` at the initial `-1`.
fn span_end_or_unclosed(end: u32, unclosed_at_eof_outer: bool) -> Value {
    if unclosed_at_eof_outer {
        json!(-1)
    } else {
        json!(end)
    }
}

/// Serialize one slot of an `{#if} / {:else if} / {:else}` chain in modern
/// AST shape. `chain_root_end` is the byte offset right after the chain's
/// outermost `{/if}`; every nested `{:else if}` IfBlock node uses this as
/// its `end` so the modern AST collapses the chain visually (vendor
/// behavior). The `is_chain_link` flag selects `elseif: true/false` for
/// nested `{:else if}` slots without depending on the AST's `elseif` field
/// (which `parse_if_block` sets only on chain links).
fn serialize_ifblock_modern_with_chain_end(
    block: &IfBlock,
    source: &str,
    in_shadow_root: bool,
    in_svelte_head: bool,
    chain_root_end: u32,
    is_chain_link: bool,
) -> Value {
    let expr_start = trimmed_span_start(block.test_span, &block.test);
    let consequent =
        serialize_fragment_modern_ctx(&block.consequent, source, in_shadow_root, in_svelte_head);
    let alternate = block.alternate.as_ref().map(|alt| {
        if let TemplateNode::IfBlock(alt_block) = alt.as_ref() {
            if alt_block.test.is_empty() && !alt_block.elseif {
                // Synthetic `{:else}` slot — serialized as a plain Fragment
                // wrapping its consequent nodes (vendor's modern AST shape).
                serialize_fragment_modern_ctx(
                    &alt_block.consequent,
                    source,
                    in_shadow_root,
                    in_svelte_head,
                )
            } else {
                // Nested `{:else if}` — recurse with the same chain root end
                // so the entire chain shares one outer `{/if}` boundary.
                let inner = serialize_ifblock_modern_with_chain_end(
                    alt_block,
                    source,
                    in_shadow_root,
                    in_svelte_head,
                    chain_root_end,
                    true,
                );
                json!({
                    "type": "Fragment",
                    "nodes": [inner]
                })
            }
        } else {
            json!(null)
        }
    });
    // Each chain link's `unclosed_at_eof_outer` flag (set by the parser
    // for non-topmost stack entries at EOF) overrides the chain-root-end
    // propagation: such links emit `-1` to match vendor's sentinel. The
    // raw `chain_root_end` value passed down stays numeric so the
    // *deepest* (topmost-in-vendor's-stack) link can still propagate
    // through to `template.length`.
    let raw_end = if is_chain_link {
        chain_root_end
    } else {
        block.span.end
    };
    json!({
        "type": "IfBlock",
        "elseif": is_chain_link,
        "start": block.span.start,
        "end": span_end_or_unclosed(raw_end, block.unclosed_at_eof_outer),
        "test": expression_to_estree(source, block.test.trim(), expr_start),
        "consequent": consequent,
        "alternate": alternate
    })
}

fn serialize_node_modern_ctx(
    node: &TemplateNode,
    source: &str,
    in_shadow_root: bool,
    in_svelte_head: bool,
) -> Value {
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
            json!({
                "type": "Comment",
                "start": c.span.start,
                "end": c.span.end,
                "data": c.data
            })
        }
        TemplateNode::Element(el) => {
            // Check if this is a <template shadowrootmode> element
            let is_shadow = el.name == "template" && el.attributes.iter().any(|a| {
                matches!(a, Attribute::NormalAttribute { name, .. } if name == "shadowrootmode")
            });
            let child_shadow = in_shadow_root || is_shadow;
            let child_in_svelte_head = el.name == "svelte:head";
            let children: Vec<Value> = el
                .children
                .iter()
                .map(|n| serialize_node_modern_ctx(n, source, child_shadow, child_in_svelte_head))
                .collect();
            let attributes: Vec<Value> = el
                .attributes
                .iter()
                .zip(el.attribute_meta.iter())
                .map(|(a, meta)| serialize_attribute_modern(a, meta, source))
                .collect();
            let el_type =
                if el.name.starts_with(|c: char| c.is_uppercase()) || el.name.contains('.') {
                    "Component"
                } else if el.name.starts_with("svelte:") {
                    match el.name.as_str() {
                        "svelte:self" => "SvelteSelf",
                        "svelte:component" => "SvelteComponent",
                        "svelte:element" => "SvelteElement",
                        "svelte:window" => "SvelteWindow",
                        "svelte:document" => "SvelteDocument",
                        "svelte:body" => "SvelteBody",
                        "svelte:head" => "SvelteHead",
                        "svelte:options" => "SvelteOptionsRaw",
                        "svelte:fragment" => "SvelteFragment",
                        "svelte:boundary" => "SvelteBoundary",
                        _ => "RegularElement",
                    }
                } else if el.name == "title" && in_svelte_head {
                    "TitleElement"
                } else if el.name == "slot" && !in_shadow_root {
                    "SlotElement"
                } else {
                    "RegularElement"
                };
            json!({
                "type": el_type,
                "start": el.span.start,
                "end": span_end_or_unclosed(el.span.end, el.unclosed_at_eof_outer),
                "name": el.name,
                "name_loc": loc_json_with_char(source, el.name_span.start, el.name_span.end),
                "attributes": attributes,
                "fragment": {
                    "type": "Fragment",
                    "nodes": children
                }
            })
        }
        TemplateNode::MustacheTag(m) => {
            let expr_start = trimmed_span_start(m.expression_span, &m.expression);
            json!({
                "type": "ExpressionTag",
                "start": m.span.start,
                "end": m.span.end,
                "expression": expression_to_estree(source, m.expression.trim(), expr_start)
            })
        }
        TemplateNode::RawMustacheTag(r) => {
            let expr_start = trimmed_span_start(r.expression_span, &r.expression);
            json!({
                "type": "HtmlTag",
                "start": r.span.start,
                "end": r.span.end,
                "expression": expression_to_estree(source, r.expression.trim(), expr_start)
            })
        }
        TemplateNode::IfBlock(block) => {
            // The chain root (`elseif: false`, non-empty test) drives every
            // `{:else if}` IfBlock's `end` to the chain's outermost `{/if}`,
            // matching vendor Svelte's modern AST (which collapses the chain
            // visually).
            serialize_ifblock_modern_with_chain_end(
                block,
                source,
                in_shadow_root,
                in_svelte_head,
                block.span.end,
                false,
            )
        }
        TemplateNode::EachBlock(block) => {
            let expr_start = trimmed_span_start(block.expression_span, &block.expression);
            let body =
                serialize_fragment_modern_ctx(&block.body, source, in_shadow_root, in_svelte_head);
            let ctx_start = block.context_span.start;
            let context_str = &block.context;
            let ctx_end = block.context_span.end;
            let context = if context_str.is_empty() {
                Value::Null
            } else if context_str.starts_with('[') || context_str.starts_with('{') {
                // Destructured pattern — parse with OXC
                let wrapper = format!("var {} = x", context_str);
                use oxc::allocator::Allocator;
                use oxc::parser::Parser;
                use oxc::span::SourceType;
                let alloc = Allocator::default();
                let result = Parser::new(&alloc, &wrapper, SourceType::mjs()).parse();
                if let Some(stmt) = result.program.body.first() {
                    if let oxc::ast::ast::Statement::VariableDeclaration(decl) = stmt {
                        if let Some(declarator) = decl.declarations.first() {
                            let mut pat = estree_binding_pat(&declarator.id, source, ctx_start - 4);
                            // Add loc and adjust columns for destructured patterns
                            if let Some(obj) = pat.as_object_mut() {
                                let s =
                                    obj.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                let e = obj.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                obj.insert("loc".to_string(), loc_json(source, s, e));
                            }
                            adjust_binding_columns(&mut pat, source);
                            pat
                        } else {
                            json!({ "type": "Identifier", "name": context_str, "start": ctx_start, "end": ctx_end })
                        }
                    } else {
                        json!({ "type": "Identifier", "name": context_str, "start": ctx_start, "end": ctx_end })
                    }
                } else {
                    json!({ "type": "Identifier", "name": context_str, "start": ctx_start, "end": ctx_end })
                }
            } else {
                json!({
                    "type": "Identifier",
                    "name": context_str,
                    "start": ctx_start,
                    "end": ctx_end,
                    "loc": loc_json_with_char(source, ctx_start, ctx_end)
                })
            };
            let mut obj = json!({
                "type": "EachBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": expression_to_estree(source, block.expression.trim(), expr_start),
                "body": body,
                "context": context
            });
            if let Some(key) = &block.key {
                if let Some(key_span) = block.key_span {
                    let key_start = trimmed_span_start(key_span, key);
                    obj["key"] = expression_to_estree(source, key.trim(), key_start);
                }
            }
            if let Some(idx) = &block.index {
                obj["index"] = json!(idx);
            }
            if let Some(fallback) = &block.fallback {
                obj["fallback"] =
                    serialize_fragment_modern_ctx(fallback, source, in_shadow_root, in_svelte_head);
            }
            obj
        }
        TemplateNode::AwaitBlock(block) => {
            let expr_start = trimmed_span_start(block.expression_span, &block.expression);
            let mut obj = json!({
                "type": "AwaitBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": expression_to_estree(source, block.expression.trim(), expr_start)
            });
            // Add pending/then/catch (always present in modern format)
            obj["pending"] = if let Some(pending) = &block.pending {
                let nodes: Vec<Value> = pending
                    .nodes
                    .iter()
                    .map(|n| serialize_node_modern_ctx(n, source, in_shadow_root, in_svelte_head))
                    .collect();
                json!({ "type": "Fragment", "nodes": nodes })
            } else {
                Value::Null
            };
            obj["then"] = if let Some(then) = &block.then {
                let nodes: Vec<Value> = then
                    .nodes
                    .iter()
                    .map(|n| serialize_node_modern_ctx(n, source, in_shadow_root, in_svelte_head))
                    .collect();
                json!({ "type": "Fragment", "nodes": nodes })
            } else {
                Value::Null
            };
            obj["catch"] = if let Some(catch) = &block.catch {
                let nodes: Vec<Value> = catch
                    .nodes
                    .iter()
                    .map(|n| serialize_node_modern_ctx(n, source, in_shadow_root, in_svelte_head))
                    .collect();
                json!({ "type": "Fragment", "nodes": nodes })
            } else {
                Value::Null
            };
            // value/error as Identifier objects
            if let Some(binding) = &block.then_binding {
                if let Some(binding_span) = block.then_binding_span {
                    let bs = trimmed_span_start(binding_span, binding);
                    let be = binding_span.end;
                    obj["value"] = json!({ "type": "Identifier", "name": binding, "start": bs, "end": be, "loc": loc_json_with_char(source, bs, be) });
                } else {
                    obj["value"] = json!(binding);
                }
            } else {
                obj["value"] = Value::Null;
            }
            if let Some(binding) = &block.catch_binding {
                if let Some(binding_span) = block.catch_binding_span {
                    let bs = trimmed_span_start(binding_span, binding);
                    let be = binding_span.end;
                    obj["error"] = json!({ "type": "Identifier", "name": binding, "start": bs, "end": be, "loc": loc_json_with_char(source, bs, be) });
                } else {
                    obj["error"] = json!(binding);
                }
            } else {
                obj["error"] = Value::Null;
            }
            obj
        }
        TemplateNode::KeyBlock(block) => {
            let expr_start = trimmed_span_start(block.expression_span, &block.expression);
            let body =
                serialize_fragment_modern_ctx(&block.body, source, in_shadow_root, in_svelte_head);
            json!({
                "type": "KeyBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": expression_to_estree(source, block.expression.trim(), expr_start),
                "fragment": body
            })
        }
        TemplateNode::SnippetBlock(block) => {
            let body =
                serialize_fragment_modern_ctx(&block.body, source, in_shadow_root, in_svelte_head);
            let (name_start, name_end) = if block.name.is_empty() {
                (block.name_span.start, block.name_span.start)
            } else {
                (block.name_span.start, block.name_span.end)
            };

            // Parse parameters
            let parameters = if !block.params.is_empty() {
                let wrapper = format!("function f({}) {{}}", block.params);
                use oxc::allocator::Allocator;
                use oxc::parser::Parser;
                use oxc::span::SourceType;
                let alloc = Allocator::default();
                let result = Parser::new(&alloc, &wrapper, SourceType::ts()).parse();
                if let Some(stmt) = result.program.body.first() {
                    if let oxc::ast::ast::Statement::FunctionDeclaration(func) = stmt {
                        let params_offset = block
                            .params_span
                            .map(|s| s.start)
                            .unwrap_or(block.name_span.end)
                            .saturating_sub(11);
                        let params: Vec<Value> = func
                            .params
                            .items
                            .iter()
                            .map(|p| estree_binding_pattern(p, source, params_offset))
                            .collect();
                        json!(params)
                    } else {
                        json!([])
                    }
                } else {
                    json!([])
                }
            } else {
                json!([])
            };

            let mut snippet_json = json!({
                "type": "SnippetBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": {
                    "type": "Identifier",
                    "name": block.name,
                    "start": name_start,
                    "end": name_end,
                    "loc": loc_json_with_char(source, name_start, name_end)
                },
                "parameters": parameters,
                "body": body
            });
            // Add typeParams for generic snippets
            if let Some(type_params) = &block.type_params {
                snippet_json["typeParams"] = json!(type_params);
            }
            snippet_json
        }
        TemplateNode::RenderTag(r) => {
            let expr_start = trimmed_span_start(r.expression_span, &r.expression);
            json!({
                "type": "RenderTag",
                "start": r.span.start,
                "end": r.span.end,
                "expression": expression_to_estree(source, r.expression.trim(), expr_start)
            })
        }
        TemplateNode::DebugTag(d) => {
            json!({
                "type": "DebugTag",
                "start": d.span.start,
                "end": d.span.end,
                "identifiers": serialize_debug_identifiers(d, source)
            })
        }
        TemplateNode::ConstTag(c) => {
            json!({
                "type": "ConstTag",
                "start": c.span.start,
                "end": c.span.end,
                "declaration": serialize_const_declaration_modern(c, source)
            })
        }
    }
}

/// Serialize a `SvelteAst` to the legacy Svelte JSON format.
pub fn to_legacy_json(ast: &SvelteAst, source: &str) -> Value {
    let has_blocks = ast.css.is_some() || ast.instance.is_some() || ast.module.is_some();
    // Find the end of the last script/style block for filtering trailing whitespace
    let last_block_end = [
        ast.instance.as_ref().map(|s| s.span.end),
        ast.module.as_ref().map(|s| s.span.end),
        ast.css.as_ref().map(|s| s.span.end),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);
    let html = serialize_fragment_legacy_root(&ast.html, source, has_blocks, last_block_end);
    let mut root = json!({ "html": html });

    // Add css if present
    if let Some(style) = &ast.css {
        root["css"] = serialize_css_legacy(style, source);
    }

    // Add instance script if present
    if let Some(script) = &ast.instance {
        let leading_comments = html_comments_before(&ast.html.nodes, script.span.start);
        root["instance"] = serialize_script_legacy(script, source, "default", &leading_comments);
    }

    // Add module script if present
    if let Some(script) = &ast.module {
        let leading_comments = html_comments_before(&ast.html.nodes, script.span.start);
        root["module"] = serialize_script_legacy(script, source, "module", &leading_comments);
    }

    // Collect all comments from scripts for root _comments field
    let mut all_comments = Vec::new();
    for script in [&ast.instance, &ast.module].into_iter().flatten() {
        let content_start = script.content_span.start;

        use oxc::allocator::Allocator;
        use oxc::parser::Parser;
        use oxc::span::SourceType;
        let alloc = Allocator::default();
        let source_type = if script.lang.as_deref() == Some("ts") {
            SourceType::ts()
        } else {
            SourceType::mjs()
        };
        let result = Parser::new(&alloc, &script.content, source_type).parse();
        for c in result.program.comments.iter() {
            let c_start = content_start + c.span.start;
            let c_end = content_start + c.span.end;
            let comment_type = if c.is_line() { "Line" } else { "Block" };
            let value = &script.content[c.span.start as usize..c.span.end as usize];
            let value = if c.is_line() {
                value.strip_prefix("//").unwrap_or(value)
            } else {
                value
                    .strip_prefix("/*")
                    .and_then(|v| v.strip_suffix("*/"))
                    .unwrap_or(value)
            };
            all_comments.push(json!({
                "type": comment_type,
                "value": value,
                "start": c_start,
                "end": c_end,
                "loc": loc_json(source, c_start, c_end)
            }));
        }
    }
    // Also collect JS comments from template expressions (inside { })
    let tmpl_script_ranges: Vec<(usize, usize)> = [&ast.instance, &ast.module]
        .iter()
        .filter_map(|s| s.as_ref())
        .map(|s| (s.span.start as usize, s.span.end as usize))
        .chain(
            ast.css
                .as_ref()
                .map(|s| (s.span.start as usize, s.span.end as usize)),
        )
        .collect();
    {
        let bytes = source.as_bytes();
        let mut i = 0;
        let mut brace_depth = 0i32;
        while i < source.len() {
            // Skip script/style blocks
            if tmpl_script_ranges.iter().any(|(s, e)| i >= *s && i < *e) {
                i += 1;
                continue;
            }
            if bytes[i] == b'{' {
                brace_depth += 1;
                i += 1;
                continue;
            }
            if bytes[i] == b'}' {
                brace_depth -= 1;
                i += 1;
                continue;
            }
            // Only look for comments INSIDE expressions
            if brace_depth > 0 {
                if i + 1 < source.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                    let start = i as u32;
                    i += 2;
                    let value_start = i;
                    while i < source.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    let value = &source[value_start..i];
                    all_comments.push(json!({
                        "type": "Line", "value": value, "start": start, "end": i as u32,
                        "loc": loc_json(source, start, i as u32)
                    }));
                    continue;
                } else if i + 1 < source.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    let start = i as u32;
                    i += 2;
                    let value_start = i;
                    while i + 1 < source.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    let value = &source[value_start..i];
                    i += 2;
                    all_comments.push(json!({
                        "type": "Block", "value": value, "start": start, "end": i as u32,
                        "loc": loc_json(source, start, i as u32)
                    }));
                    continue;
                }
            }
            // Skip strings
            if bytes[i] == b'"' || bytes[i] == b'\'' || bytes[i] == b'`' {
                let q = bytes[i];
                i += 1;
                while i < source.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < source.len() {
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
    }
    // Sort all_comments by start position
    all_comments.sort_by(|a, b| {
        let a_start = a.get("start").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_start = b.get("start").and_then(|v| v.as_u64()).unwrap_or(0);
        a_start.cmp(&b_start)
    });

    if !all_comments.is_empty() {
        root["_comments"] = json!(all_comments);
    }

    // Convert byte offsets to character offsets for sources with multi-byte characters
    if has_multibyte(source) {
        convert_byte_to_char_offsets(&mut root, source);
    }

    root
}

fn serialize_css_legacy(style: &Style, _source: &str) -> Value {
    let content_start = style.content_span.start;
    let content_end = style.content_span.end;

    // Parse CSS children and strip internal fields
    let mut children = crate::parser::css::parse_css_children(&style.content, content_start);
    strip_internal_fields(&mut children);

    json!({
        "type": "Style",
        "start": style.span.start,
        "end": style.span.end,
        "attributes": [],
        "children": children,
        "content": {
            "start": content_start,
            "end": content_end,
            "styles": style.content,
            "comment": null
        }
    })
}

fn html_comments_before<'a>(nodes: &'a [TemplateNode<'a>], before: u32) -> Vec<&'a Comment> {
    let mut expected_end = before;

    for node in nodes.iter().rev() {
        if node_span_start(node) >= before {
            continue;
        }
        if node_span_end(node) != expected_end {
            break;
        }

        match node {
            TemplateNode::Comment(comment) => return vec![comment],
            TemplateNode::Text(text) if text.data.trim().is_empty() => {
                expected_end = text.span.start;
            }
            _ => break,
        }
    }

    Vec::new()
}

fn serialize_script_legacy(
    script: &Script,
    source: &str,
    context: &str,
    leading_html_comments: &[&Comment],
) -> Value {
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

    let content_start = script.content_span.start;

    let result = Parser::new(&alloc, &script.content, source_type).parse();

    let program_end = script.content_span.end;

    // Compute loc using the actual source line of the <script> tag
    let (start_line, _) = offset_to_loc(source, script.span.start as usize);
    let (end_line, end_col) = offset_to_loc(source, script.span.end as usize);
    let content_loc = json!({
        "start": { "line": start_line, "column": 0 },
        "end": { "line": end_line, "column": end_col }
    });

    // Serialize the program body statements with comment association
    let mut body: Vec<Value> = result
        .program
        .body
        .iter()
        .map(|stmt| serialize_statement_legacy(stmt, source, content_start))
        .collect();

    // Associate comments with statements using attached_to
    for c in result.program.comments.iter() {
        let c_start = content_start + c.span.start;
        let c_end = content_start + c.span.end;
        let comment_type = if c.is_line() { "Line" } else { "Block" };
        let raw = &script.content[c.span.start as usize..c.span.end as usize];
        let value = if c.is_line() {
            raw.strip_prefix("//").unwrap_or(raw)
        } else {
            raw.strip_prefix("/*")
                .and_then(|v| v.strip_suffix("*/"))
                .unwrap_or(raw)
        };
        let comment_json = json!({
            "type": comment_type,
            "value": value,
            "start": c_start,
            "end": c_end
        });

        // Find the node this comment is attached to
        let attached_abs = content_start + c.attached_to;
        if c.is_leading() {
            // Leading comment: attached_to points to the target node's start
            // First try top-level body statements
            let mut found = false;
            for stmt in body.iter_mut() {
                let stmt_start = stmt.get("start").and_then(|s| s.as_u64()).unwrap_or(0) as u32;
                if stmt_start == attached_abs {
                    if let Some(obj) = stmt.as_object_mut() {
                        let arr = obj.entry("leadingComments").or_insert(json!([]));
                        if let Some(a) = arr.as_array_mut() {
                            a.push(comment_json.clone());
                        }
                    }
                    found = true;
                    break;
                }
            }
            // If not found at top level, recursively search nested nodes
            if !found {
                for stmt in body.iter_mut() {
                    if attach_comment_recursive(stmt, &comment_json, attached_abs, c_start, source)
                    {
                        break;
                    }
                }
            }
        } else {
            // Trailing comment: find the statement that ends just before this comment on the same line
            let mut found = false;
            // First try top-level body statements
            for (i, stmt) in body.iter().enumerate() {
                let stmt_end = stmt.get("end").and_then(|s| s.as_u64()).unwrap_or(0) as u32;
                if stmt_end <= c_start && (c_start - stmt_end) <= 2 {
                    let stmt_end_line = offset_to_loc(source, stmt_end as usize).0;
                    let comment_line = offset_to_loc(source, c_start as usize).0;
                    if stmt_end_line == comment_line {
                        if let Some(obj) = body[i].as_object_mut() {
                            let arr = obj.entry("trailingComments").or_insert(json!([]));
                            if let Some(a) = arr.as_array_mut() {
                                a.push(comment_json.clone());
                            }
                        }
                        found = true;
                        break;
                    }
                }
            }
            if found {
                continue;
            }
            // Try to attach to nested nodes by walking the body tree
            for stmt in body.iter_mut() {
                if attach_comment_recursive(stmt, &comment_json, attached_abs, c_start, source) {
                    break;
                }
            }
        }
    }

    // Serialize comments
    let comments: Vec<Value> = result
        .program
        .comments
        .iter()
        .map(|c| {
            let c_start = content_start + c.span.start;
            let c_end = content_start + c.span.end;
            let comment_type = if c.is_line() { "Line" } else { "Block" };
            let value = &script.content[c.span.start as usize..c.span.end as usize];
            // Strip the comment delimiters
            let value = if c.is_line() {
                value.strip_prefix("//").unwrap_or(value)
            } else {
                value
                    .strip_prefix("/*")
                    .and_then(|v| v.strip_suffix("*/"))
                    .unwrap_or(value)
            };
            json!({
                "type": comment_type,
                "value": value,
                "start": c_start,
                "end": c_end
            })
        })
        .collect();

    let mut program = json!({
        "type": "Program",
        "start": content_start,
        "end": program_end,
        "loc": content_loc,
        "body": body,
        "sourceType": "module"
    });

    // Only add trailingComments to Program if there are no statements to attach to
    if !comments.is_empty() && body.is_empty() {
        program["trailingComments"] = json!(comments);
    }

    let leading: Vec<Value> = leading_html_comments
        .iter()
        .map(|comment| {
            json!({
                "type": "Line",
                "value": comment.data
            })
        })
        .collect();
    if !leading.is_empty() {
        program["leadingComments"] = json!(leading);
    }

    json!({
        "type": "Script",
        "start": script.span.start,
        "end": script.span.end,
        "context": context,
        "content": program
    })
}

#[allow(dead_code)]
fn offset_to_loc_json(text: &str, offset: usize) -> Value {
    let (line, col) = offset_to_loc(text, offset);
    json!({ "line": line, "column": col })
}

/// Serialize a JS statement to legacy estree JSON.
fn serialize_statement_legacy(
    stmt: &oxc::ast::ast::Statement<'_>,
    source: &str,
    offset: u32,
) -> Value {
    use oxc::ast::ast::Statement;
    match stmt {
        Statement::VariableDeclaration(decl) => {
            let start = offset + decl.span.start;
            let end = offset + decl.span.end;
            let declarations: Vec<Value> = decl
                .declarations
                .iter()
                .map(|d| {
                    let d_start = offset + d.span.start;
                    let d_end = offset + d.span.end;
                    let mut id = estree_binding_pat(&d.id, source, offset);
                    let init = d.init.as_ref().map(|e| estree_expr(e, source, offset));
                    // Add typeAnnotation from VariableDeclarator to the id
                    if let Some(type_ann) = &d.type_annotation {
                        if let Some(id_obj) = id.as_object_mut() {
                            let ann_start = offset + type_ann.span.start;
                            let ann_end = offset + type_ann.span.end;
                            // Extend id end to include type annotation
                            id_obj.insert("end".to_string(), json!(ann_end));
                            let type_node =
                                serialize_ts_type(&type_ann.type_annotation, source, offset);
                            id_obj.insert(
                                "typeAnnotation".to_string(),
                                json!({
                                    "type": "TSTypeAnnotation",
                                    "start": ann_start,
                                    "end": ann_end,
                                    "loc": loc_json(source, ann_start, ann_end),
                                    "typeAnnotation": type_node
                                }),
                            );
                            // Update loc end
                            if let Some(loc) = id_obj.get_mut("loc") {
                                if let Some(loc_obj) = loc.as_object_mut() {
                                    let (el, ec) = offset_to_loc(source, ann_end as usize);
                                    loc_obj.insert(
                                        "end".to_string(),
                                        json!({"line": el, "column": ec}),
                                    );
                                }
                            }
                        }
                    }
                    json!({
                        "type": "VariableDeclarator",
                        "start": d_start,
                        "end": d_end,
                        "loc": loc_json(source, d_start, d_end),
                        "id": id,
                        "init": init
                    })
                })
                .collect();
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

            let specifiers: Vec<Value> = imp.specifiers.as_ref().map(|specs| {
                specs.iter().map(|spec| {
                    use oxc::ast::ast::ImportDeclarationSpecifier;
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            let s_start = offset + s.span.start;
                            let s_end = offset + s.span.end;
                            let local_start = offset + s.local.span.start;
                            let local_end = offset + s.local.span.end;
                            let imported_span = match &s.imported {
                                oxc::ast::ast::ModuleExportName::IdentifierName(id) => id.span,
                                oxc::ast::ast::ModuleExportName::IdentifierReference(id) => id.span,
                                oxc::ast::ast::ModuleExportName::StringLiteral(s) => s.span,
                            };
                            let imported_name = match &s.imported {
                                oxc::ast::ast::ModuleExportName::IdentifierName(id) => id.name.as_str(),
                                oxc::ast::ast::ModuleExportName::IdentifierReference(id) => id.name.as_str(),
                                oxc::ast::ast::ModuleExportName::StringLiteral(s) => s.value.as_str(),
                            };
                            json!({
                                "type": "ImportSpecifier",
                                "start": s_start,
                                "end": s_end,
                                "loc": loc_json(source, s_start, s_end),
                                "imported": {
                                    "type": "Identifier",
                                    "start": offset + imported_span.start,
                                    "end": offset + imported_span.end,
                                    "loc": loc_json(source, offset + imported_span.start, offset + imported_span.end),
                                    "name": imported_name
                                },
                                "local": {
                                    "type": "Identifier",
                                    "start": local_start,
                                    "end": local_end,
                                    "loc": loc_json(source, local_start, local_end),
                                    "name": s.local.name.as_str()
                                }
                            })
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            let s_start = offset + s.span.start;
                            let s_end = offset + s.span.end;
                            json!({
                                "type": "ImportDefaultSpecifier",
                                "start": s_start,
                                "end": s_end,
                                "loc": loc_json(source, s_start, s_end),
                                "local": {
                                    "type": "Identifier",
                                    "start": offset + s.local.span.start,
                                    "end": offset + s.local.span.end,
                                    "name": s.local.name.as_str()
                                }
                            })
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            let s_start = offset + s.span.start;
                            let s_end = offset + s.span.end;
                            json!({
                                "type": "ImportNamespaceSpecifier",
                                "start": s_start,
                                "end": s_end,
                                "local": {
                                    "type": "Identifier",
                                    "start": offset + s.local.span.start,
                                    "end": offset + s.local.span.end,
                                    "name": s.local.name.as_str()
                                }
                            })
                        }
                    }
                }).collect()
            }).unwrap_or_default();

            let s_start = offset + imp.source.span.start;
            let s_end = offset + imp.source.span.end;
            let source_val = json!({
                "type": "Literal",
                "start": s_start,
                "end": s_end,
                "loc": loc_json(source, s_start, s_end),
                "value": imp.source.value.as_str(),
                "raw": &source[s_start as usize..s_end as usize]
            });

            json!({
                "type": "ImportDeclaration",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "specifiers": specifiers,
                "source": source_val,
                "attributes": []
            })
        }
        Statement::ExportNamedDeclaration(exp) => {
            let start = offset + exp.span.start;
            let end = offset + exp.span.end;
            let declaration = exp
                .declaration
                .as_ref()
                .map(|d| serialize_statement_legacy_from_decl(d, source, offset));
            json!({
                "type": "ExportNamedDeclaration",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "declaration": declaration,
                "specifiers": [],
                "source": null,
                "attributes": []
            })
        }
        Statement::ReturnStatement(ret) => {
            let start = offset + ret.span.start;
            let end = offset + ret.span.end;
            json!({
                "type": "ReturnStatement",
                "start": start,
                "end": end,
                "loc": loc_json(source, start, end),
                "argument": ret.argument.as_ref().map(|a| estree_expr(a, source, offset))
            })
        }
        Statement::FunctionDeclaration(f) => {
            let start = offset + f.span.start;
            let end = offset + f.span.end;
            let params: Vec<Value> = f
                .params
                .items
                .iter()
                .map(|p| estree_binding_pattern(p, source, offset))
                .collect();
            let body_val = f.body.as_ref().map(|b| {
                let b_start = offset + b.span.start;
                let b_end = offset + b.span.end;
                let stmts: Vec<Value> = b
                    .statements
                    .iter()
                    .map(|s| serialize_statement_legacy(s, source, offset))
                    .collect();
                json!({
                    "type": "BlockStatement",
                    "start": b_start,
                    "end": b_end,
                    "loc": loc_json(source, b_start, b_end),
                    "body": stmts
                })
            });
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
                }),
                "expression": false,
                "generator": f.generator,
                "async": f.r#async,
                "params": params,
                "body": body_val
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

fn serialize_statement_legacy_from_decl(
    decl: &oxc::ast::ast::Declaration<'_>,
    source: &str,
    offset: u32,
) -> Value {
    use oxc::ast::ast::Declaration;
    match decl {
        Declaration::VariableDeclaration(v) => {
            // Reuse the VariableDeclaration serialization by wrapping in a Statement
            let start = offset + v.span.start;
            let end = offset + v.span.end;
            let declarations: Vec<Value> = v
                .declarations
                .iter()
                .map(|d| {
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
                })
                .collect();
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
        _ => json!({ "type": "UnknownDeclaration" }),
    }
}

fn serialize_fragment_legacy_root(
    fragment: &Fragment,
    source: &str,
    has_blocks: bool,
    last_block_end: u32,
) -> Value {
    // If the fragment has no nodes at all (script-only file with no whitespace between blocks)
    if fragment.nodes.is_empty() && has_blocks {
        return json!({
            "type": "Fragment",
            "start": null,
            "end": null,
            "children": []
        });
    }

    // For root with script/style blocks: keep nodes but strip trailing whitespace after last block
    // only if there's no non-whitespace content after it
    let filtered = if has_blocks {
        let mut nodes: Vec<&TemplateNode> = fragment.nodes.iter().collect();
        // Only strip the LAST node if it's whitespace after the last block
        while let Some(last) = nodes.last() {
            if let TemplateNode::Text(t) = last {
                if t.data.chars().all(|c| c.is_ascii_whitespace()) && t.span.start >= last_block_end
                {
                    nodes.pop();
                    continue;
                }
            }
            break;
        }
        nodes
    } else {
        strip_trailing_whitespace(&fragment.nodes)
    };
    let children: Vec<Value> = filtered
        .iter()
        .map(|n| serialize_node_legacy(n, source))
        .collect();
    // Fragment end: use the last NON-whitespace child's end. Vendor's
    // legacy converter does the same thing (`legacy.js` reads
    // `node.fragment.nodes.at(-1).end` for the html span). When that
    // child carries the `unclosed_at_eof_outer` flag — set by drain
    // when an outer (non-topmost) entry is unwound at EOF — vendor
    // reports `end: -1` (its sentinel for a never-closed node), and so
    // do we.
    let last_non_ws = filtered.iter().rev().find(
        |n| !matches!(n, TemplateNode::Text(t) if t.data.chars().all(|c| c.is_ascii_whitespace())),
    );
    let last_unclosed = last_non_ws
        .map(|n| node_unclosed_at_eof_outer(n))
        .unwrap_or(false);
    let mut end = last_non_ws
        .map(|n| node_span_end(n))
        .or_else(|| filtered.last().map(|n| node_span_end(n)))
        .unwrap_or(fragment.span.end);
    // If source ends with newline and end == source.len(), trim it
    if end as usize == source.len() && source.ends_with('\n') {
        end -= 1;
    }
    let has_non_whitespace = filtered.iter().any(
        |n| !matches!(n, TemplateNode::Text(t) if t.data.chars().all(|c| c.is_ascii_whitespace())),
    );
    let start;
    if has_non_whitespace {
        start = filtered.iter()
            .find(|n| !matches!(n, TemplateNode::Text(t) if t.data.chars().all(|c| c.is_ascii_whitespace())))
            .map(|n| node_span_start(n))
            .unwrap_or(fragment.span.start);
    } else if has_blocks && !filtered.is_empty() {
        // Only whitespace between scripts — inverted range (Svelte compiler behavior)
        start = filtered
            .last()
            .map(|n| node_span_end(n))
            .unwrap_or(fragment.span.start);
        end = filtered.first().map(|n| node_span_start(n)).unwrap_or(end);
    } else {
        start = filtered
            .first()
            .map(|n| node_span_start(n))
            .unwrap_or(fragment.span.start);
    }
    json!({
        "type": "Fragment",
        "start": start,
        "end": span_end_or_unclosed(end, last_unclosed),
        "children": children
    })
}

/// True iff the AST node is an `Element` or block kind that carries the
/// `unclosed_at_eof_outer` flag and the flag is set. Used by serializers
/// to decide whether to emit `-1` for the node's `end` (matching vendor
/// Svelte's sentinel for non-topmost in-stack nodes left over at EOF).
fn node_unclosed_at_eof_outer(node: &TemplateNode) -> bool {
    match node {
        TemplateNode::Element(e) => e.unclosed_at_eof_outer,
        TemplateNode::IfBlock(b) => b.unclosed_at_eof_outer,
        TemplateNode::EachBlock(b) => b.unclosed_at_eof_outer,
        TemplateNode::AwaitBlock(b) => b.unclosed_at_eof_outer,
        TemplateNode::KeyBlock(b) => b.unclosed_at_eof_outer,
        TemplateNode::SnippetBlock(b) => b.unclosed_at_eof_outer,
        _ => false,
    }
}

/// Replicate vendor `legacy.js`'s `source.lastIndexOf('{', node.end - 1)`
/// for the legacy IfBlock / ElseBlock anchor computation. Pass `-1` for
/// `end_value` to get vendor's behavior when the IfBlock's `end` would
/// have been the `-1` sentinel (left there by the parser stack on EOF
/// for outer chain links and chain roots) — JS `lastIndexOf` with
/// negative `fromIndex` coerces to `0`, returning `0` when source[0] is
/// `{` (always true at a chain root).
fn legacy_lastindexof_open_brace(source: &str, end_value: i64) -> u32 {
    let from_index = end_value - 1;
    if from_index < 0 {
        return 0;
    }
    let bytes = source.as_bytes();
    let limit = (from_index as usize).min(bytes.len().saturating_sub(1));
    bytes[..=limit]
        .iter()
        .rposition(|&b| b == b'{')
        .map(|i| i as u32)
        .unwrap_or(0)
}

/// The byte offset vendor's modern AST uses for an IfBlock's `end`,
/// reproducing vendor's parser-stack rules:
/// - The chain root (`elseif: false`) keeps its own `node.end`
///   (or vendor's `-1` sentinel when unclosed at EOF).
/// - Chain links (`elseif: true`) collapse to the outer end via the
///   `tag.js` close-loop on a successful `{/if}`; on EOF without a close
///   only the topmost in vendor's stack is adjusted to `template.length`,
///   the others keep `-1`. `unclosed_at_eof_outer` discriminates these.
///
/// `outer_end` is the chain root's `span.end`, used for the "closed
/// chain collapse" branch.
fn legacy_ifblock_effective_end(block: &IfBlock, outer_end: u32) -> i64 {
    if block.unclosed_at_eof_outer {
        -1
    } else if block.elseif {
        outer_end as i64
    } else {
        block.span.end as i64
    }
}

#[allow(dead_code)]
fn serialize_fragment_legacy(fragment: &Fragment, source: &str) -> Value {
    // Root fragment: only strip trailing whitespace, keep all other nodes
    let filtered = strip_trailing_whitespace(&fragment.nodes);
    let children: Vec<Value> = filtered
        .iter()
        .map(|n| serialize_node_legacy(n, source))
        .collect();
    let end = filtered
        .last()
        .map(|n| node_span_end(n))
        .unwrap_or(fragment.span.end);
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
    serialize_node_legacy_ctx(node, source, false)
}

fn serialize_node_legacy_ctx(node: &TemplateNode, source: &str, in_svelte_head: bool) -> Value {
    match node {
        TemplateNode::Text(t) => serialize_text_legacy(t, true),
        TemplateNode::Comment(c) => {
            // Parse svelte-ignore directives from comment text
            let ignores: Vec<&str> = if c.data.trim_start().starts_with("svelte-ignore") {
                let after_prefix = c
                    .data
                    .trim_start()
                    .strip_prefix("svelte-ignore")
                    .unwrap_or("");
                after_prefix
                    .split(',')
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
            let child_in_svelte_head = el.name == "svelte:head";
            let children: Vec<Value> = el
                .children
                .iter()
                .map(|n| {
                    if el.name.eq_ignore_ascii_case("style") {
                        if let TemplateNode::Text(text) = n {
                            return serialize_text_legacy(text, false);
                        }
                    }
                    serialize_node_legacy_ctx(n, source, child_in_svelte_head)
                })
                .collect();
            // For svelte:component, extract the `this` attribute as `expression`
            let mut extra_fields = serde_json::Map::new();
            let mut filtered_attrs: Vec<(&Attribute, &AttributeMeta)> =
                el.attributes.iter().zip(el.attribute_meta.iter()).collect();
            // For svelte:element, the field name is "tag" instead of "expression"
            let this_field_name = if el.name == "svelte:element" {
                "tag"
            } else {
                "expression"
            };

            if el.name == "svelte:component" || el.name == "svelte:element" {
                if let Some(idx) = filtered_attrs.iter().position(|(attr, _)| {
                    matches!(attr, Attribute::NormalAttribute { name, .. } if name == "this")
                }) {
                    let (this_attr, this_meta) = filtered_attrs.remove(idx);
                    if let Attribute::NormalAttribute { value, span, .. } = this_attr {
                        match value {
                            AttributeValue::Expression(expr) => {
                                let expression_span = this_meta.expression_span.unwrap_or(*span);
                                extra_fields.insert(this_field_name.to_string(),
                                    expression_to_estree(
                                        source,
                                        expr.trim(),
                                        trimmed_span_start(expression_span, expr),
                                    ));
                            }
                            AttributeValue::Static(s) => {
                                let inner = s.trim();
                                // Plain string value: this="div"
                                extra_fields.insert(this_field_name.to_string(), json!(inner));
                            }
                            AttributeValue::Concat(parts) => {
                                // Single expression in concat: ="{expr}"
                                if parts.len() == 1 {
                                    if let AttributeValuePart::Expression(expr) = &parts[0] {
                                        let expression_span = this_meta
                                            .parts
                                            .first()
                                            .and_then(|meta| meta.expression_span)
                                            .unwrap_or(*span);
                                        extra_fields.insert(this_field_name.to_string(),
                                            expression_to_estree(
                                                source,
                                                expr.trim(),
                                                trimmed_span_start(expression_span, expr),
                                            ));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            let el_type =
                if el.name.starts_with(|c: char| c.is_uppercase()) || el.name.contains('.') {
                    "InlineComponent"
                } else if el.name.starts_with("svelte:") {
                    match el.name.as_str() {
                        "svelte:self" => "InlineComponent",
                        "svelte:component" => "InlineComponent",
                        "svelte:element" => "Element",
                        "svelte:window" => "Window",
                        "svelte:document" => "Document",
                        "svelte:body" => "Body",
                        "svelte:head" => "Head",
                        "svelte:options" => "Options",
                        "svelte:fragment" => "SlotTemplate",
                        _ => "Element",
                    }
                } else if el.name == "title" && in_svelte_head {
                    "Title"
                } else if el.name == "slot" {
                    "Slot"
                } else {
                    "Element"
                };
            let attributes: Vec<Value> = filtered_attrs
                .iter()
                .map(|(attr, meta)| serialize_attribute_legacy(attr, meta, source))
                .collect();
            // For <style> elements inside other elements, if empty, add empty Text node
            let children = if el.name == "style" && children.is_empty() {
                let content_pos = el.start_tag_end + 1;
                vec![json!({
                    "type": "Text",
                    "start": content_pos,
                    "end": content_pos,
                    "data": ""
                })]
            } else {
                children
            };
            let mut obj = json!({
                "type": el_type,
                "start": el.span.start,
                "end": span_end_or_unclosed(el.span.end, el.unclosed_at_eof_outer),
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
            let trimmed = m.expression.trim();
            let expr_start = trimmed_span_start(m.expression_span, &m.expression);
            json!({
                "type": "MustacheTag",
                "start": m.span.start,
                "end": m.span.end,
                "expression": expression_to_estree(source, trimmed, expr_start)
            })
        }
        TemplateNode::RawMustacheTag(r) => {
            let expr_start = trimmed_span_start(r.expression_span, &r.expression);
            json!({
                "type": "RawMustacheTag",
                "start": r.span.start,
                "end": r.span.end,
                "expression": expression_to_estree(source, r.expression.trim(), expr_start)
            })
        }
        TemplateNode::DebugTag(d) => {
            json!({
                "type": "DebugTag",
                "start": d.span.start,
                "end": d.span.end,
                "identifiers": serialize_debug_identifiers(d, source)
            })
        }
        TemplateNode::ConstTag(c) => {
            json!({
                "type": "ConstTag",
                "start": c.span.start,
                "end": c.span.end,
                "expression": serialize_const_expression_legacy(c, source)
            })
        }
        TemplateNode::RenderTag(r) => {
            let expr_start = trimmed_span_start(r.expression_span, &r.expression);
            json!({
                "type": "RenderTag",
                "start": r.span.start,
                "end": r.span.end,
                "expression": expression_to_estree(source, r.expression.trim(), expr_start)
            })
        }
        TemplateNode::IfBlock(block) => {
            let (children, _) = serialize_filtered_children_ctx(
                &block.consequent.nodes,
                source,
                block.span.end,
                in_svelte_head,
            );
            let expr_start = trimmed_span_start(block.test_span, &block.test);
            let mut obj = json!({
                "type": "IfBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": expression_to_estree(source, block.test.trim(), expr_start),
                "children": children
            });
            if let Some(alt) = &block.alternate {
                match alt.as_ref() {
                    TemplateNode::IfBlock(alt_block) => {
                        // Vendor's legacy converter computes the ElseBlock's
                        // `end` via `source.lastIndexOf('{', node.end - 1)`
                        // (where `node` is the *outer* IfBlock containing
                        // the alternate) and its `start` via
                        // `nodes.at(0)?.start ?? end`. The same algorithm
                        // works for both closed chains and unclosed-at-EOF
                        // chains — when the outer is unclosed the effective
                        // end is `-1` and `lastIndexOf` returns `0`; when
                        // it's closed, `lastIndexOf` finds the `{` of the
                        // closing `{/if}`, which always matches what
                        // `parse_if_block`'s span recorded. Replacing the
                        // old `(alt_block.span.start, alt_block.span.end)`
                        // shortcut with the lastIndexOf algorithm fixes the
                        // unclosed-`{#if a}{:else}` case where the
                        // synthetic-else's `body_start` (after the `}` of
                        // `{:else}`) doesn't match vendor's `{` anchor.
                        let outer_effective_end =
                            legacy_ifblock_effective_end(block, block.span.end);
                        let else_anchor =
                            legacy_lastindexof_open_brace(source, outer_effective_end);
                        if alt_block.test.is_empty() && !alt_block.elseif {
                            // {:else} block
                            let (else_children, _) = serialize_filtered_children_ctx(
                                &alt_block.consequent.nodes,
                                source,
                                alt_block.span.end,
                                in_svelte_head,
                            );
                            let else_start = alt_block
                                .consequent
                                .nodes
                                .first()
                                .map(node_span_start)
                                .unwrap_or(else_anchor);
                            obj["else"] = json!({
                                "type": "ElseBlock",
                                "start": else_start,
                                "end": else_anchor,
                                "children": else_children
                            });
                        } else {
                            // {:else if ...} block — wrap IfBlock in an ElseBlock.
                            let inner = serialize_elseif_block(
                                alt_block,
                                source,
                                block.span.end,
                                in_svelte_head,
                            );
                            let else_start = alt_block
                                .consequent
                                .nodes
                                .first()
                                .map(node_span_start)
                                .unwrap_or(else_anchor);
                            obj["else"] = json!({
                                "type": "ElseBlock",
                                "start": else_start,
                                "end": else_anchor,
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
            let (children, _) = serialize_filtered_children_ctx(
                &block.body.nodes,
                source,
                block.span.end,
                in_svelte_head,
            );
            let expr_start = trimmed_span_start(block.expression_span, &block.expression);
            let context_str = &block.context;

            let ctx_start = block.context_span.start;
            let ctx_end = block.context_span.end;

            // Parse context - could be Identifier, ArrayPattern, ObjectPattern, or null for empty
            let context = if context_str.is_empty() {
                Value::Null
            } else if context_str.starts_with('[') || context_str.starts_with('{') {
                // Destructured pattern — parse with oxc
                // Wrap in a var declaration to parse as a pattern
                let wrapper = format!("var {} = x", context_str);
                use oxc::allocator::Allocator;
                use oxc::parser::Parser;
                use oxc::span::SourceType;
                let alloc = Allocator::default();
                let result = Parser::new(&alloc, &wrapper, SourceType::mjs()).parse();
                if let Some(stmt) = result.program.body.first() {
                    if let oxc::ast::ast::Statement::VariableDeclaration(decl) = stmt {
                        if let Some(declarator) = decl.declarations.first() {
                            // Offset: ctx_start minus the "var " prefix (4 chars)
                            let mut pat = estree_binding_pat(&declarator.id, source, ctx_start - 4);
                            adjust_binding_columns(&mut pat, source);
                            pat
                        } else {
                            json!({ "type": "Identifier", "name": context_str, "start": ctx_start, "end": ctx_end })
                        }
                    } else {
                        json!({ "type": "Identifier", "name": context_str, "start": ctx_start, "end": ctx_end })
                    }
                } else {
                    json!({ "type": "Identifier", "name": context_str, "start": ctx_start, "end": ctx_end })
                }
            } else {
                json!({
                    "type": "Identifier",
                    "name": context_str,
                    "start": ctx_start,
                    "end": ctx_end,
                    "loc": loc_json_with_char(source, ctx_start, ctx_end)
                })
            };

            let mut obj = json!({
                "type": "EachBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "children": children,
                "context": context,
                "expression": expression_to_estree(source, block.expression.trim(), expr_start)
            });

            if let Some(idx) = &block.index {
                obj["index"] = json!(idx);
            }

            if let Some(key) = &block.key {
                if let Some(key_span) = block.key_span {
                    let key_start = trimmed_span_start(key_span, key);
                    obj["key"] = expression_to_estree(source, key.trim(), key_start);
                }
            }

            if let Some(fb) = &block.fallback {
                let (else_children, _) =
                    serialize_filtered_children_ctx(&fb.nodes, source, fb.span.end, in_svelte_head);
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
            let expr_start = trimmed_span_start(block.expression_span, &block.expression);
            let expr_end = block.expression_span.end;
            let mut obj = json!({
                "type": "AwaitBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": expression_to_estree(source, block.expression.trim(), expr_start)
            });

            // value (then binding) — serialize as Identifier or null
            if let Some(binding) = &block.then_binding {
                if let Some(binding_span) = block.then_binding_span {
                    let binding_start = trimmed_span_start(binding_span, binding);
                    let binding_end = binding_span.end;
                    obj["value"] = json!({
                        "type": "Identifier",
                        "name": binding,
                        "start": binding_start,
                        "end": binding_end,
                        "loc": loc_json_with_char(source, binding_start, binding_end)
                    });
                } else {
                    obj["value"] = json!(binding);
                }
            } else {
                obj["value"] = Value::Null;
            }

            // error (catch binding)
            if let Some(binding) = &block.catch_binding {
                if let Some(binding_span) = block.catch_binding_span {
                    let binding_start = trimmed_span_start(binding_span, binding);
                    let binding_end = binding_span.end;
                    obj["error"] = json!({
                        "type": "Identifier",
                        "name": binding,
                        "start": binding_start,
                        "end": binding_end,
                        "loc": loc_json_with_char(source, binding_start, binding_end)
                    });
                } else {
                    obj["error"] = json!(binding);
                }
            } else {
                obj["error"] = Value::Null;
            }

            // Pending/then/catch spans intentionally follow Svelte's legacy AST adapter.
            let pending_end = if let Some(pending) = &block.pending {
                let children: Vec<Value> = pending
                    .nodes
                    .iter()
                    .map(|n| serialize_node_legacy_ctx(n, source, in_svelte_head))
                    .collect();
                let pending_start = pending
                    .nodes
                    .first()
                    .map(node_span_start)
                    .unwrap_or_else(|| index_after_next_closing_brace(source, expr_end));
                let pending_end = pending
                    .nodes
                    .last()
                    .map(node_span_end)
                    .unwrap_or(pending_start);
                obj["pending"] = json!({
                    "type": "PendingBlock",
                    "start": pending_start,
                    "end": pending_end,
                    "children": children,
                    "skip": false
                });
                Some(pending_end)
            } else {
                obj["pending"] = json!({
                    "type": "PendingBlock",
                    "start": null,
                    "end": null,
                    "children": [],
                    "skip": true
                });
                None
            };

            let then_end = if let Some(then) = &block.then {
                let children: Vec<Value> = then
                    .nodes
                    .iter()
                    .map(|n| serialize_node_legacy_ctx(n, source, in_svelte_head))
                    .collect();
                let then_start = pending_end
                    .or_else(|| then.nodes.first().map(node_span_start))
                    .unwrap_or_else(|| index_after_next_closing_brace(source, expr_end));
                let then_end = then.nodes.last().map(node_span_end).unwrap_or_else(|| {
                    index_after_last_closing_brace_at_or_before(
                        source,
                        pending_end.unwrap_or(expr_end),
                    )
                });
                obj["then"] = json!({
                    "type": "ThenBlock",
                    "start": then_start,
                    "end": then_end,
                    "children": children,
                    "skip": false
                });
                Some(then_end)
            } else {
                obj["then"] = json!({
                    "type": "ThenBlock",
                    "start": null,
                    "end": null,
                    "children": [],
                    "skip": true
                });
                None
            };

            if let Some(catch) = &block.catch {
                let children: Vec<Value> = catch
                    .nodes
                    .iter()
                    .map(|n| serialize_node_legacy_ctx(n, source, in_svelte_head))
                    .collect();
                let catch_start = then_end
                    .or(pending_end)
                    .or_else(|| catch.nodes.first().map(node_span_start))
                    .unwrap_or_else(|| index_after_next_closing_brace(source, expr_end));
                let catch_end = catch.nodes.last().map(node_span_end).unwrap_or_else(|| {
                    index_after_last_closing_brace_at_or_before(
                        source,
                        then_end.or(pending_end).unwrap_or(expr_end),
                    )
                });
                obj["catch"] = json!({
                    "type": "CatchBlock",
                    "start": catch_start,
                    "end": catch_end,
                    "children": children,
                    "skip": false
                });
            } else {
                obj["catch"] = json!({
                    "type": "CatchBlock",
                    "start": null,
                    "end": null,
                    "children": [],
                    "skip": true
                });
            }

            obj
        }
        TemplateNode::KeyBlock(block) => {
            let (children, _) = serialize_filtered_children_ctx(
                &block.body.nodes,
                source,
                block.span.end,
                in_svelte_head,
            );
            let expr_start = trimmed_span_start(block.expression_span, &block.expression);
            json!({
                "type": "KeyBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": expression_to_estree(source, block.expression.trim(), expr_start),
                "children": children
            })
        }
        TemplateNode::SnippetBlock(block) => {
            let (children, _) = serialize_filtered_children_ctx(
                &block.body.nodes,
                source,
                block.span.end,
                in_svelte_head,
            );

            let (name_start, name_end) = if block.name.is_empty() {
                (block.name_span.start, block.name_span.start)
            } else {
                (block.name_span.start, block.name_span.end)
            };

            let expression = json!({
                "type": "Identifier",
                "name": block.name,
                "start": name_start,
                "end": name_end,
                "loc": loc_json_with_char(source, name_start, name_end)
            });

            // Parse parameters if present
            let parameters = if !block.params.is_empty() {
                // Parse as function params using oxc
                let wrapper = format!("function f({}) {{}}", block.params);
                use oxc::allocator::Allocator;
                use oxc::parser::Parser;
                use oxc::span::SourceType;
                let alloc = Allocator::default();
                let result = Parser::new(&alloc, &wrapper, SourceType::ts()).parse();
                if let Some(stmt) = result.program.body.first() {
                    if let oxc::ast::ast::Statement::FunctionDeclaration(func) = stmt {
                        let params_offset = block
                            .params_span
                            .map(|s| s.start)
                            .unwrap_or(block.name_span.end)
                            .saturating_sub(11);
                        let params: Vec<Value> = func
                            .params
                            .items
                            .iter()
                            .map(|p| estree_binding_pattern(p, source, params_offset))
                            .collect();
                        json!(params)
                    } else {
                        json!([])
                    }
                } else {
                    json!([])
                }
            } else {
                json!([])
            };

            let mut obj = json!({
                "type": "SnippetBlock",
                "start": block.span.start,
                "end": span_end_or_unclosed(block.span.end, block.unclosed_at_eof_outer),
                "expression": expression,
                "parameters": parameters,
                "children": children
            });

            // Add typeParams for generic snippets
            if let Some(type_params) = &block.type_params {
                obj["typeParams"] = json!(type_params);
            }

            obj
        }
    }
}

fn serialize_text_legacy(text: &Text, include_raw: bool) -> Value {
    let mut value = json!({
        "type": "Text",
        "start": text.span.start,
        "end": text.span.end,
        "data": decode_entities(&text.data)
    });
    if include_raw {
        value["raw"] = json!(text.data);
    }
    value
}

fn serialize_debug_identifiers(debug: &DebugTag, source: &str) -> Vec<Value> {
    debug
        .identifiers
        .iter()
        .zip(debug.identifier_spans.iter())
        .map(|(name, span)| {
            json!({
                "type": "Identifier",
                "start": span.start,
                "end": span.end,
                "loc": loc_json(source, span.start, span.end),
                "name": name
            })
        })
        .collect()
}

fn serialize_const_declaration_modern(const_tag: &ConstTag, source: &str) -> Value {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let wrapper = format!("const {}", const_tag.declaration);
    let offset = const_tag.span.start + 2;
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, &wrapper, SourceType::ts()).parse();

    let Some(stmt) = parsed.program.body.first() else {
        return Value::Null;
    };
    let mut declaration = serialize_statement_legacy(stmt, source, offset);

    if let Some(obj) = declaration.as_object_mut() {
        obj.remove("loc");
        if let Some(declarations) = obj.get_mut("declarations").and_then(|v| v.as_array_mut()) {
            for declarator in declarations {
                if let Some(declarator_obj) = declarator.as_object_mut() {
                    declarator_obj.remove("loc");
                    if let Some(id) = declarator_obj.get_mut("id") {
                        add_character_to_locs(id, source);
                    }
                }
            }
        }
    }

    declaration
}

fn serialize_const_expression_legacy(const_tag: &ConstTag, source: &str) -> Value {
    let expr_start = trimmed_span_start(const_tag.declaration_span, &const_tag.declaration);
    let mut expression = expression_to_estree(source, const_tag.declaration.trim(), expr_start);
    if let Some(obj) = expression.as_object_mut() {
        obj.remove("loc");
        if let Some(left) = obj.get_mut("left") {
            add_character_to_locs(left, source);
        }
    }
    expression
}

fn add_character_to_locs(value: &mut Value, source: &str) {
    match value {
        Value::Object(obj) => {
            let start = obj.get("start").and_then(|v| v.as_u64()).map(|v| v as u32);
            let end = obj.get("end").and_then(|v| v.as_u64()).map(|v| v as u32);
            if obj.contains_key("loc") {
                if let (Some(start), Some(end)) = (start, end) {
                    obj.insert("loc".to_string(), loc_json_with_char(source, start, end));
                }
            }
            for child in obj.values_mut() {
                add_character_to_locs(child, source);
            }
        }
        Value::Array(values) => {
            for child in values {
                add_character_to_locs(child, source);
            }
        }
        _ => {}
    }
}

/// Serialize an {:else if} IfBlock with elseif:true flag.
/// `outer_end` is the chain root's `span.end` (used to mirror vendor's
/// modern-AST chain-end collapse for closed chains; ignored when the
/// block carries the `unclosed_at_eof_outer` flag).
fn serialize_elseif_block(
    block: &IfBlock,
    source: &str,
    outer_end: u32,
    in_svelte_head: bool,
) -> Value {
    let (children, _) = serialize_filtered_children_ctx(
        &block.consequent.nodes,
        source,
        block.span.end,
        in_svelte_head,
    );
    let expr_start = trimmed_span_start(block.test_span, &block.test);

    // Vendor: `consequent.nodes.at(0)?.start ?? lastIndexOf('{', node.end - 1)`.
    let block_effective_end = legacy_ifblock_effective_end(block, outer_end);
    let content_start = block
        .consequent
        .nodes
        .first()
        .map(node_span_start)
        .unwrap_or_else(|| legacy_lastindexof_open_brace(source, block_effective_end));

    let mut obj = json!({
        "type": "IfBlock",
        "start": content_start,
        "end": if block_effective_end < 0 { json!(-1) } else { json!(block_effective_end as u32) },
        "expression": expression_to_estree(source, block.test.trim(), expr_start),
        "children": children,
        "elseif": true
    });

    // Handle nested alternates. The current block (`block`) is the outer
    // for these; its effective end determines the ElseBlock anchor via
    // vendor's `legacy.js`'s `source.lastIndexOf('{', outer.end - 1)`.
    if let Some(alt) = &block.alternate {
        if let TemplateNode::IfBlock(alt_block) = alt.as_ref() {
            let else_anchor = legacy_lastindexof_open_brace(source, block_effective_end);
            let else_start = alt_block
                .consequent
                .nodes
                .first()
                .map(node_span_start)
                .unwrap_or(else_anchor);
            if alt_block.test.is_empty() && !alt_block.elseif {
                // {:else} block
                let (else_children, _) = serialize_filtered_children_ctx(
                    &alt_block.consequent.nodes,
                    source,
                    alt_block.span.end,
                    in_svelte_head,
                );
                obj["else"] = json!({
                    "type": "ElseBlock",
                    "start": else_start,
                    "end": else_anchor,
                    "children": else_children
                });
            } else {
                // Nested {:else if ...}
                let inner = serialize_elseif_block(alt_block, source, outer_end, in_svelte_head);
                obj["else"] = json!({
                    "type": "ElseBlock",
                    "start": else_start,
                    "end": else_anchor,
                    "children": [inner]
                });
            }
        }
    }

    obj
}

fn serialize_attribute_legacy(attr: &Attribute, meta: &AttributeMeta, source: &str) -> Value {
    match attr {
        Attribute::NormalAttribute { name, value, span } => {
            // Check for shorthand attribute: {name}
            let is_shorthand = meta.mustache_span == Some(*span)
                && matches!(value, AttributeValue::Expression(e) if e == name);

            if is_shorthand {
                // Shorthand: {id} → Attribute with AttributeShorthand value
                let expr_start = meta
                    .expression_span
                    .map(|s| s.start)
                    .unwrap_or(span.start + 1);
                let expr_end = meta.expression_span.map(|s| s.end).unwrap_or(span.end - 1);
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
                let n_start = meta.name_span.start;
                let n_end = meta.name_span.end;

                let value_json = serialize_attr_value_legacy(value, meta, source);

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
            let expression_span = meta.expression_span.unwrap_or(*span);
            let expr_str = source_for_span(source, expression_span);
            let expr_start = trimmed_span_start(expression_span, expr_str);
            json!({
                "type": "Spread",
                "start": span.start,
                "end": span.end,
                "expression": expression_to_estree(source, expr_str.trim(), expr_start)
            })
        }
        Attribute::Directive {
            kind,
            name,
            modifiers,
            value,
            span,
        } => {
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

            // Parse expression from directive value if present
            let expression = directive_expression_legacy(value, meta, source);

            let mut obj = json!({
                "start": span.start,
                "end": span.end,
                "type": type_name,
                "name": name,
                "name_loc": loc_json_with_char(source, meta.name_span.start, meta.name_span.end),
                "modifiers": modifiers
            });

            // Style directives use "value" instead of "expression"
            if matches!(kind, DirectiveKind::StyleDirective) {
                if let Some(expr) = expression {
                    if expr.is_array() {
                        // Already an array (e.g., string value [{Text}]) — use directly
                        obj["value"] = expr;
                    } else {
                        // Expression value — wrap in MustacheTag array
                        if let Some(mustache_span) = meta
                            .mustache_span
                            .or_else(|| meta.parts.first().and_then(|part| part.mustache_span))
                        {
                            obj["value"] = json!([{
                                "type": "MustacheTag",
                                "start": mustache_span.start,
                                "end": mustache_span.end,
                                "expression": expr
                            }]);
                        } else {
                            obj["value"] = json!([expr]);
                        }
                    }
                } else {
                    obj["value"] = json!(true);
                }
            } else {
                if let Some(expr) = expression {
                    obj["expression"] = expr;
                } else if matches!(kind, DirectiveKind::Binding) {
                    // Shorthand binding: bind:foo → expression is Identifier("foo")
                    let subject_span = meta.directive_subject_span.unwrap_or(meta.name_span);
                    obj["expression"] = json!({
                        "type": "Identifier",
                        "start": subject_span.start,
                        "end": subject_span.end,
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

fn serialize_attr_value_legacy(
    value: &AttributeValue,
    meta: &AttributeMeta,
    source: &str,
) -> Value {
    match value {
        AttributeValue::True => json!(true),
        AttributeValue::Static(s) => {
            let value_span = meta.value_span.unwrap_or(meta.name_span);
            json!([{
                "start": value_span.start,
                "end": value_span.end,
                "type": "Text",
                "raw": s,
                "data": decode_entities(s)
            }])
        }
        AttributeValue::Expression(expr) => {
            let expression_span = meta.expression_span.unwrap_or(meta.name_span);
            let mustache_span = meta.mustache_span.unwrap_or(expression_span);
            let expr_start = trimmed_span_start(expression_span, expr);
            json!([{
                "type": "MustacheTag",
                "start": mustache_span.start,
                "end": mustache_span.end,
                "expression": expression_to_estree(source, expr.trim(), expr_start)
            }])
        }
        AttributeValue::Concat(parts) => {
            let values: Vec<Value> = parts
                .iter()
                .zip(meta.parts.iter())
                .map(|(part, part_meta)| match part {
                    AttributeValuePart::Static(s) => {
                        json!({
                            "start": part_meta.span.start,
                            "end": part_meta.span.end,
                            "type": "Text",
                            "raw": s,
                            "data": decode_entities(s)
                        })
                    }
                    AttributeValuePart::Expression(expr) => {
                        let expression_span = part_meta.expression_span.unwrap_or(part_meta.span);
                        let mustache_span = part_meta.mustache_span.unwrap_or(part_meta.span);
                        let expr_start = trimmed_span_start(expression_span, expr);
                        json!({
                            "type": "MustacheTag",
                            "start": mustache_span.start,
                            "end": mustache_span.end,
                            "expression": expression_to_estree(source, expr.trim(), expr_start)
                        })
                    }
                })
                .collect();
            Value::Array(values)
        }
    }
}
