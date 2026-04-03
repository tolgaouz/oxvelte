//! `svelte/prefer-class-directive` — prefer class directives over ternary class attributes.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart};

pub struct PreferClassDirective;

impl Rule for PreferClassDirective {
    fn name(&self) -> &'static str {
        "svelte/prefer-class-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let prefer_empty = ctx.config.options.as_ref().and_then(|v| v.as_array()).and_then(|a| a.first())
            .and_then(|v| v.get("prefer")).and_then(|v| v.as_str()).map(|s| s != "always").unwrap_or(true);
        let msg = "Unexpected class using the ternary operator.";

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            if el.name.as_bytes().first().map_or(false, |c| c.is_ascii_uppercase()) { return; }
            if el.name.starts_with("svelte:") && el.name != "svelte:element" { return; }

            for attr in &el.attributes {
                let Attribute::NormalAttribute { name, value, span } = attr else { continue };
                if name != "class" { continue; }
                match value {
                    AttributeValue::Expression(expr) => {
                        if is_simple_class_ternary(expr) || (!prefer_empty && is_dual_class_ternary(expr)) {
                            ctx.diagnostic(msg, *span);
                        }
                    }
                    AttributeValue::Concat(parts) => {
                        let boundary_ok = |parts: &[AttributeValuePart], idx: usize, before: bool| -> bool {
                            let adj = if before { idx.checked_sub(1) } else { (idx < parts.len() - 1).then(|| idx + 1) };
                            adj.map_or(true, |j| match &parts[j] {
                                AttributeValuePart::Static(s) => if before { s.is_empty() || s.ends_with(' ') } else { s.is_empty() || s.starts_with(' ') },
                                _ => false,
                            })
                        };
                        for (i, part) in parts.iter().enumerate() {
                            let AttributeValuePart::Expression(expr) = part else { continue };
                            if is_simple_class_ternary(expr) && boundary_ok(parts, i, true) && boundary_ok(parts, i, false) {
                                ctx.diagnostic(msg, *span);
                            } else if !prefer_empty && is_dual_class_ternary(expr) {
                                let ok = parts.iter().enumerate().all(|(j, p)| j == i || match p {
                                    AttributeValuePart::Expression(e) => e.trim().ends_with(": ''") || e.trim().ends_with(": \"\"") || !e.contains('?'),
                                    _ => true,
                                });
                                if ok { ctx.diagnostic(msg, *span); }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

fn is_dual_class_ternary(expr: &str) -> bool {
    split_ternary(expr.trim()).map_or(false, |(_, t, f)| is_single_class_name(t) && is_single_class_name(f))
}

fn is_single_class_name(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 3 { return false; }
    let inner = if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) { &s[1..s.len()-1] } else { return false; };
    !inner.trim().is_empty() && !inner.trim().contains(' ')
}

fn unquote_str(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.len() < 2 { return None; }
    let b = s.as_bytes();
    if (b[0] == b'\'' && b[s.len()-1] == b'\'') || (b[0] == b'"' && b[s.len()-1] == b'"') { return Some(&s[1..s.len()-1]); }
    if b[0] == b'`' && b[s.len()-1] == b'`' { let i = &s[1..s.len()-1]; if !i.contains("${") { return Some(i); } }
    None
}

fn split_ternary(expr: &str) -> Option<(&str, &str, &str)> {
    let bytes = expr.as_bytes();
    let (mut depth, mut q, mut c, mut i) = (0i32, None, None, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'\'' | b'"' | b'`' => { let qt = bytes[i]; i += 1; while i < bytes.len() { if bytes[i] == b'\\' { i += 1; } else if bytes[i] == qt { break; } i += 1; } }
            b'?' if depth == 0 && q.is_none() => q = Some(i),
            b':' if depth == 0 && q.is_some() && c.is_none() => c = Some(i),
            _ => {}
        }
        i += 1;
    }
    let (q, c) = (q?, c?);
    Some((expr[..q].trim(), expr[q+1..c].trim(), expr[c+1..].trim()))
}

fn is_simple_class_ternary(expr: &str) -> bool {
    let Some((_, tb, fb)) = split_ternary(expr.trim()) else { return false; };
    if let (Some(ti), Some(fi)) = (unquote_str(tb), unquote_str(fb)) {
        let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
        (fi.trim().is_empty() && valid(ti.trim())) || (ti.trim().is_empty() && valid(fi.trim()))
    } else { false }
}
