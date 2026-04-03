//! `svelte/no-not-function-handler` — disallow non-function event handlers.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use oxc::span::Span;
use std::collections::HashMap;

pub struct NoNotFunctionHandler;

fn is_non_function_literal(expr: &str) -> bool {
    let s = expr.trim();
    matches!(s, "true" | "false" | "undefined")
        || s.parse::<f64>().is_ok()
        || (s.ends_with('n') && s[..s.len()-1].parse::<i64>().is_ok())
        || matches!(s.as_bytes(), [b'"', .., b'"'] | [b'\'', .., b'\''] | [b'`', .., b'`']
            | [b'[', .., b']'] | [b'{', .., b'}'])
        || s.starts_with("class ") || s.starts_with("new ")
        || (s.starts_with('/') && s.len() > 1 && s[1..].rfind('/').map_or(false, |e| s[e+2..].chars().all(|c| "gimsuy".contains(c))))
}

fn non_function_phrase(expr: &str) -> &'static str {
    let s = expr.trim();
    match s.as_bytes().first() {
        Some(b'[') => "array", Some(b'{') => "object",
        Some(b'"' | b'\'' | b'`') => "string value", Some(b'/') => "regex value",
        _ if s == "true" || s == "false" => "boolean value",
        _ if s.starts_with("new ") => "new expression",
        _ if s.starts_with("class ") => "class",
        _ if s.ends_with('n') && s[..s.len()-1].parse::<i64>().is_ok() => "bigint value",
        _ if s.parse::<f64>().is_ok() => "number value",
        _ => "non-function value",
    }
}

fn find_non_function_vars(content: &str) -> HashMap<String, &'static str> {
    let mut vars = HashMap::new();
    for line in content.lines() {
        let t = line.trim();
        for kw in &["const ", "let ", "var "] {
            if let Some(rest) = t.strip_prefix(kw) {
                let ne = rest.find(|c: char| c == '=' || c == ' ' || c == ':').unwrap_or(rest.len());
                let name = rest[..ne].trim();
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') { continue; }
                if let Some(eq) = rest.find('=') {
                    let init = rest[eq + 1..].trim().strip_suffix(';').unwrap_or(rest[eq + 1..].trim()).trim();
                    if !init.is_empty() && is_non_function_literal(init) { vars.insert(name.to_string(), non_function_phrase(init)); }
                }
            }
        }
    }
    vars
}

fn check_handler_value(ctx: &mut LintContext, non_fn_vars: &HashMap<String, &'static str>, span: Span) {
    let region = &ctx.source[span.start as usize..span.end as usize];
    let Some(eq_pos) = region.find('=') else { return };
    let value = region[eq_pos + 1..].trim();
    if !value.starts_with('{') || !value.ends_with('}') { return; }
    let expr = value[1..value.len()-1].trim();
    let bo = span.start as usize + region.find('{').unwrap_or(eq_pos + 1);
    let es = &ctx.source[bo + 1..span.end as usize];
    let ts = es.len() - es.trim_start().len();
    let expr_span = Span::new((bo + 1 + ts) as u32, (bo + 1 + ts + expr.len()) as u32);

    if is_non_function_literal(expr) {
        let phrase = non_function_phrase(expr);
        ctx.diagnostic(format!("Unexpected {} in event handler.", phrase), expr_span);
    } else if expr.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
        if let Some(phrase) = non_fn_vars.get(expr) {
            ctx.diagnostic(format!("Unexpected {} in event handler.", phrase), expr_span);
        }
    }
}

impl Rule for NoNotFunctionHandler {
    fn name(&self) -> &'static str {
        "svelte/no-not-function-handler"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let vars = ctx.ast.instance.as_ref().map(|s| find_non_function_vars(&s.content)).unwrap_or_default();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            for attr in &el.attributes {
                let span = match attr {
                    Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } => *span,
                    Attribute::NormalAttribute { name, span, .. }
                        if name.starts_with("on") && name.len() > 2 && name.as_bytes()[2].is_ascii_lowercase() => *span,
                    _ => continue,
                };
                check_handler_value(ctx, &vars, span);
            }
        });
    }
}
