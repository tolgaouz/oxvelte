//! `svelte/no-goto-without-base` — require goto to use base path.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{Expression, ImportDeclarationSpecifier, ModuleExportName, Statement};
use oxc::ast::AstKind;
use oxc::span::{GetSpan, Span};

pub struct NoGotoWithoutBase;

impl Rule for NoGotoWithoutBase {
    fn name(&self) -> &'static str {
        "svelte/no-goto-without-base"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;
        let program = semantic.nodes().program();

        // Resolve the local names bound to `goto` (from `$app/navigation`) and
        // `base` (from `$app/paths`). Namespace imports resolve to `ns.goto`
        // and `ns.base` for call-callee matching.
        let mut goto_names: Vec<String> = Vec::new();
        let mut base_name: Option<String> = None;
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
                        if src == "$app/navigation" && imported_name == "goto" {
                            goto_names.push(s.local.name.to_string());
                        }
                        if src == "$app/paths" && imported_name == "base" {
                            base_name = Some(s.local.name.to_string());
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        if src == "$app/navigation" {
                            goto_names.push(format!("{}.goto", s.local.name));
                        }
                        if src == "$app/paths" {
                            base_name = Some(format!("{}.base", s.local.name));
                        }
                    }
                    _ => {}
                }
            }
        }
        if goto_names.is_empty() {
            return;
        }

        for node in semantic.nodes().iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            let callee_text = callee_static_name(&ce.callee);
            let Some(callee_text) = callee_text else { continue };
            if !goto_names.iter().any(|g| g == &callee_text) {
                continue;
            }
            let Some(first_arg) = ce.arguments.first().and_then(|a| a.as_expression()) else {
                continue;
            };
            // `goto(base + '/foo/')` / `` goto(`${base}/foo/`) `` — prefix use is fine.
            if let Some(bn) = &base_name {
                if arg_uses_base(first_arg, bn) {
                    continue;
                }
            }
            // Only flag arguments we can analyze as path literals (string, template,
            // or binary `+` chains starting with one). Dynamic arguments like
            // `goto(someVar)` or `goto(getPath())` are left alone.
            let Some(leading_path) = leading_string_prefix(first_arg) else {
                continue;
            };
            if is_absolute_url(&leading_path) {
                continue;
            }
            let callee_span = ce.callee.span();
            let s = content_offset + callee_span.start;
            let e = content_offset + callee_span.end + 1; // include `(`
            ctx.diagnostic(
                "Use `base` from `$app/paths` when calling `goto` with an absolute path.",
                Span::new(s, e),
            );
        }
    }
}

/// Return the leading static string prefix of a path-like expression. Returns
/// `None` if we can't statically determine a prefix (dynamic expression).
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

/// Extract the static name of a call's callee: `foo` or `ns.foo`. Returns None
/// for computed accesses, call chains, etc.
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

fn is_absolute_url(path: &str) -> bool {
    path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("mailto:")
        || path.starts_with("tel:")
        || path.starts_with("//")
}

/// Does the argument expression use `base` as a PREFIX (not just somewhere)?
/// Only the leftmost position counts — `'/foo/' + base` and `` `/foo/${base}` ``
/// are NOT prefixed and should still be flagged.
fn arg_uses_base(expr: &Expression<'_>, base_name: &str) -> bool {
    match expr {
        Expression::TemplateLiteral(t) => {
            // Base-prefixed template: first quasi is empty and first interpolation is base.
            if let (Some(first_quasi), Some(first_expr)) = (t.quasis.first(), t.expressions.first()) {
                let first_text = first_quasi.value.cooked.as_deref().unwrap_or(first_quasi.value.raw.as_str());
                if first_text.is_empty() && is_base_ref(first_expr, base_name) {
                    return true;
                }
            }
            false
        }
        Expression::BinaryExpression(b) if b.operator == oxc::syntax::operator::BinaryOperator::Addition => {
            // Base-prefixed concat: leftmost operand is base (recursively).
            arg_uses_base(&b.left, base_name) || is_base_ref(&b.left, base_name)
        }
        _ => is_base_ref(expr, base_name),
    }
}

/// Is this expression a direct reference to `base`?
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
