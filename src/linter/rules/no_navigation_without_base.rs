//! `svelte/no-navigation-without-base` — require navigation functions to use base path.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, TemplateNode};
use oxc::ast::ast::{Expression, ImportDeclarationSpecifier, ModuleExportName, Statement};
use oxc::ast::AstKind;
use oxc::span::{GetSpan, Span};

const NAV_FUNCTIONS: &[&str] = &["goto", "pushState", "replaceState"];

pub struct NoNavigationWithoutBase;

fn is_nav_ignored(name: &str, ignore_goto: bool, ignore_push_state: bool, ignore_replace_state: bool) -> bool {
    match name {
        "goto" => ignore_goto,
        "pushState" => ignore_push_state,
        "replaceState" => ignore_replace_state,
        _ => false,
    }
}

fn is_exempt_href(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
        || s.starts_with("mailto:") || s.starts_with("tel:")
        || s.starts_with("//") || s.starts_with('#')
        || s.is_empty()
}

impl Rule for NoNavigationWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array()).and_then(|arr| arr.first());
        let get_bool = |key: &str| opts.and_then(|v| v.get(key)).and_then(|v| v.as_bool()).unwrap_or(false);
        let ignore_goto = get_bool("ignoreGoto");
        let ignore_push_state = get_bool("ignorePushState");
        let ignore_replace_state = get_bool("ignoreReplaceState");
        let ignore_links = get_bool("ignoreLinks");

        // Resolve local names for base + nav functions from the instance script.
        let mut base_name: Option<String> = None;
        let mut nav_locals: Vec<(String, &'static str)> = Vec::new(); // (local-call-text, original-name)
        if let Some(semantic) = ctx.instance_semantic {
            let program = semantic.nodes().program();
            for stmt in &program.body {
                let Statement::ImportDeclaration(imp) = stmt else { continue };
                let src = imp.source.value.as_str();
                let Some(specifiers) = &imp.specifiers else { continue };
                for spec in specifiers {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            let imported_name = match &s.imported {
                                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                                ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                                ModuleExportName::StringLiteral(l) => l.value.as_str(),
                            };
                            if src == "$app/paths" && imported_name == "base" {
                                base_name = Some(s.local.name.to_string());
                            }
                            if src == "$app/navigation" {
                                if let Some(nav) = NAV_FUNCTIONS.iter().find(|f| **f == imported_name) {
                                    if !is_nav_ignored(nav, ignore_goto, ignore_push_state, ignore_replace_state) {
                                        nav_locals.push((s.local.name.to_string(), nav));
                                    }
                                }
                            }
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            if src == "$app/paths" {
                                base_name = Some(format!("{}.base", s.local.name));
                            }
                            if src == "$app/navigation" {
                                for nav in NAV_FUNCTIONS {
                                    if is_nav_ignored(nav, ignore_goto, ignore_push_state, ignore_replace_state) {
                                        continue;
                                    }
                                    nav_locals.push((format!("{}.{}", s.local.name, nav), nav));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Walk call expressions in the instance script.
        if let Some(semantic) = ctx.instance_semantic {
            if !nav_locals.is_empty() {
                let content_offset = ctx.instance_content_offset;
                for node in semantic.nodes().iter() {
                    let AstKind::CallExpression(ce) = node.kind() else { continue };
                    let Some(callee_text) = callee_static_name(&ce.callee) else { continue };
                    let Some((_, orig_name)) = nav_locals.iter().find(|(l, _)| l == &callee_text) else { continue };

                    let Some(first_arg) = ce.arguments.first().and_then(|a| a.as_expression()) else { continue };
                    if let Some(bn) = &base_name {
                        if arg_uses_base(first_arg, bn) {
                            continue;
                        }
                    }
                    let Some(leading) = leading_string_prefix(first_arg) else { continue };
                    if is_exempt_href(&leading) {
                        continue;
                    }
                    let callee_span = ce.callee.span();
                    let s = content_offset + callee_span.start;
                    let e = content_offset + callee_span.end + 1; // include `(`
                    ctx.diagnostic(
                        format!("Found a {}() call with a url that isn't prefixed with the base path.", orig_name),
                        Span::new(s, e),
                    );
                }
            }
        }

        if ignore_links { return; }

        // Anchor href checks — template-based (still uses the template AST + raw
        // string extraction for attribute values because template expressions
        // aren't in the semantic model).
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "a" { return; }
                for attr in &el.attributes {
                    if let Attribute::NormalAttribute { name, span, .. } = attr {
                        if name != "href" { continue; }
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(eq_pos) = region.find('=') {
                            let val = region[eq_pos + 1..].trim();
                            if matches!((val.as_bytes().first(), val.as_bytes().last()),
                                (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\''))) {
                                let inner = &val[1..val.len()-1];
                                if inner.starts_with('/') && !is_exempt_href(inner) {
                                    ctx.diagnostic("Found a link with a url that isn't prefixed with the base path.", *span);
                                }
                            }
                            else if val.starts_with('{') && val.ends_with('}') {
                                let expr = val[1..val.len()-1].trim();

                                let uses_base = if let Some(ref bname) = base_name {
                                    expr.starts_with(&format!("{} +", bname))
                                    || expr.starts_with(&format!("{}+", bname))
                                    || expr.starts_with(&format!("${{{}}}",  bname))
                                    || expr.starts_with(&format!("`${{{}}}", bname))
                                } else { false };

                                if uses_base { continue; }

                                let is_path_literal = matches!(expr.as_bytes().first(), Some(b'\'' | b'"' | b'`'))
                                    && expr[1..].find(expr.as_bytes()[0] as char)
                                        .map_or(false, |e| expr[1..e+1].starts_with('/'));

                                let has_path_concat = expr.contains("'/'") || expr.contains("\"/\"");

                                if is_path_literal || has_path_concat {
                                    ctx.diagnostic("Found a link with a url that isn't prefixed with the base path.", *span);
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

fn callee_static_name(callee: &Expression<'_>) -> Option<String> {
    match callee {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::StaticMemberExpression(mem) => {
            if let Expression::Identifier(id) = &mem.object {
                Some(format!("{}.{}", id.name, mem.property.name))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn arg_uses_base(expr: &Expression<'_>, base_name: &str) -> bool {
    match expr {
        Expression::TemplateLiteral(t) => {
            if let (Some(first_quasi), Some(first_expr)) = (t.quasis.first(), t.expressions.first()) {
                let first_text = first_quasi.value.cooked.as_deref().unwrap_or(first_quasi.value.raw.as_str());
                if first_text.is_empty() && is_base_ref(first_expr, base_name) {
                    return true;
                }
            }
            false
        }
        Expression::BinaryExpression(b) if b.operator == oxc::syntax::operator::BinaryOperator::Addition => {
            arg_uses_base(&b.left, base_name) || is_base_ref(&b.left, base_name)
        }
        _ => is_base_ref(expr, base_name),
    }
}

fn is_base_ref(expr: &Expression<'_>, base_name: &str) -> bool {
    match expr {
        Expression::Identifier(id) => id.name == base_name,
        Expression::StaticMemberExpression(mem) => {
            if let Expression::Identifier(id) = &mem.object {
                let composed = format!("{}.{}", id.name, mem.property.name);
                composed == base_name
            } else {
                false
            }
        }
        _ => false,
    }
}

fn leading_string_prefix(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(l) => Some(l.value.to_string()),
        Expression::TemplateLiteral(t) => t
            .quasis
            .first()
            .map(|q| q.value.cooked.as_deref().unwrap_or(q.value.raw.as_str()).to_string()),
        Expression::BinaryExpression(b) if b.operator == oxc::syntax::operator::BinaryOperator::Addition => {
            leading_string_prefix(&b.left)
        }
        _ => None,
    }
}
