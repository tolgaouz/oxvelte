//! `svelte/no-navigation-without-resolve` — disallow SvelteKit navigation calls
//! (`goto`, `pushState`, etc.) without using `$app/paths` `resolveRoute`.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, TemplateNode};
use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Expression, ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc::ast::AstKind;
use oxc::parser::Parser;
use oxc::semantic::Semantic;
use oxc::span::{GetSpan, Ident, SourceType, Span};
use rustc_hash::FxHashSet;

const NAV_FUNCTIONS: &[&str] = &["goto", "pushState", "replaceState"];

pub struct NoNavigationWithoutResolve;

impl Rule for NoNavigationWithoutResolve {
    fn name(&self) -> &'static str {
        "svelte/no-navigation-without-resolve"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref().and_then(|v| v.as_array()).and_then(|arr| arr.first());
        let get_bool = |key: &str| opts.and_then(|v| v.get(key)).and_then(|v| v.as_bool()).unwrap_or(false);
        let ignore_goto = get_bool("ignoreGoto");
        let ignore_push_state = get_bool("ignorePushState");
        let ignore_replace_state = get_bool("ignoreReplaceState");
        let ignore_links = get_bool("ignoreLinks");

        // Resolve import locals.
        let mut nav_locals: Vec<(String, &'static str)> = Vec::new(); // (local-callable, original)
        let mut resolve_locals: Vec<String> = Vec::new();
        let mut has_sveltekit_paths = false;
        let mut has_any_imports = false;

        if let Some(sem) = ctx.instance_semantic {
            for stmt in &sem.nodes().program().body {
                let Statement::ImportDeclaration(imp) = stmt else { continue };
                has_any_imports = true;
                let src = imp.source.value.as_str();
                let is_nav_mod = src == "$app/navigation";
                let is_paths_mod = src == "$app/paths";
                if is_paths_mod {
                    has_sveltekit_paths = true;
                }
                let Some(specifiers) = &imp.specifiers else { continue };
                for spec in specifiers {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            let imported = match &s.imported {
                                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                                ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                                ModuleExportName::StringLiteral(l) => l.value.as_str(),
                            };
                            if is_nav_mod {
                                if let Some(nav) = NAV_FUNCTIONS.iter().find(|f| **f == imported) {
                                    if !is_nav_ignored(nav, ignore_goto, ignore_push_state, ignore_replace_state) {
                                        nav_locals.push((s.local.name.to_string(), nav));
                                    }
                                }
                            }
                            if is_paths_mod && matches!(imported, "resolve" | "asset") {
                                resolve_locals.push(s.local.name.to_string());
                            }
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            if is_nav_mod {
                                for nav in NAV_FUNCTIONS {
                                    if is_nav_ignored(nav, ignore_goto, ignore_push_state, ignore_replace_state) {
                                        continue;
                                    }
                                    nav_locals.push((format!("{}.{}", s.local.name, nav), nav));
                                }
                            }
                            if is_paths_mod {
                                resolve_locals.push(format!("{}.resolve", s.local.name));
                                resolve_locals.push(format!("{}.asset", s.local.name));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Walk script nav calls.
        if !nav_locals.is_empty() {
            if let Some(sem) = ctx.instance_semantic {
                let content_offset = ctx.instance_content_offset;
                for node in sem.nodes().iter() {
                    let AstKind::CallExpression(ce) = node.kind() else { continue };
                    let Some(callee_text) = callee_static_name(&ce.callee) else { continue };
                    let Some((_, orig_name)) = nav_locals.iter().find(|(l, _)| l == &callee_text) else { continue };
                    let Some(first_arg) = ce.arguments.first().and_then(|a| a.as_expression()) else { continue };

                    let safe = is_safe_nav_arg(first_arg, &resolve_locals, sem, &mut FxHashSet::default());
                    if !safe {
                        let callee_span = ce.callee.span();
                        let s = content_offset + callee_span.start;
                        let e = content_offset + callee_span.end + 1;
                        ctx.diagnostic(
                            format!("Unexpected {}() call without resolve().", orig_name),
                            Span::new(s, e),
                        );
                    }
                }
            }
        }

        if ignore_links {
            return;
        }

        // Template anchor href checks.
        // Skip entirely for non-SvelteKit files (fast bail).
        if has_any_imports && !has_sveltekit_paths && nav_locals.is_empty() {
            // No `$app/*` imports at all — definitely not a SvelteKit routing context.
            return;
        }

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name != "a" {
                    return;
                }
                // `rel="external"` opts-out.
                let has_external = el.attributes.iter().any(|a| matches!(
                    a,
                    Attribute::NormalAttribute { name, value, .. } if name == "rel" && match value {
                        AttributeValue::Static(v) => v.contains("external"),
                        AttributeValue::Expression(e) => e.contains("external") || e.trim() == "rel",
                        _ => false,
                    }
                ));
                if has_external {
                    return;
                }

                for attr in &el.attributes {
                    let Attribute::NormalAttribute { name, value, span, .. } = attr else { continue };
                    if name != "href" {
                        continue;
                    }

                    let ok = match value {
                        AttributeValue::Static(v) => is_exempt_href(v),
                        AttributeValue::Expression(expr_text) => {
                            // Fast path: textual indicators of absolute/fragment
                            // URLs. Matches vendor's coarse heuristic — covers
                            // concatenations and template literals that build
                            // URLs from pieces.
                            let trimmed = expr_text.trim();
                            if expr_text.contains("://")
                                || trimmed.starts_with("'#")
                                || trimmed.starts_with("\"#")
                                || trimmed.starts_with("`#")
                            {
                                true
                            } else {
                                is_safe_template_expr(expr_text, &resolve_locals, ctx.instance_semantic)
                            }
                        }
                        AttributeValue::True => true,
                        AttributeValue::Concat(_) => true,
                    };
                    if !ok {
                        ctx.diagnostic("Unexpected href link without resolve().", *span);
                    }
                }
            }
        });
    }
}

fn is_nav_ignored(name: &str, ignore_goto: bool, ignore_push_state: bool, ignore_replace_state: bool) -> bool {
    match name {
        "goto" => ignore_goto,
        "pushState" => ignore_push_state,
        "replaceState" => ignore_replace_state,
        _ => false,
    }
}

fn is_exempt_href(s: &str) -> bool {
    s.is_empty()
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("mailto:")
        || s.starts_with("tel:")
        || s.starts_with("//")
        || s.starts_with('#')
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

/// Is this a call to resolve/asset (or aliased/namespaced variant)?
fn is_resolve_call(expr: &Expression<'_>, resolve_locals: &[String]) -> bool {
    let Expression::CallExpression(ce) = expr else { return false };
    let Some(text) = callee_static_name(&ce.callee) else { return false };
    resolve_locals.iter().any(|r| r == &text)
}

/// Script-side: is the nav-call's first argument safe?
fn is_safe_nav_arg<'a>(
    expr: &'a Expression<'a>,
    resolve_locals: &[String],
    semantic: &'a Semantic<'a>,
    seen: &mut FxHashSet<oxc::semantic::SymbolId>,
) -> bool {
    match expr {
        Expression::CallExpression(_) => is_resolve_call(expr, resolve_locals),
        Expression::StringLiteral(l) => is_exempt_href(l.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            let first = t.quasis.first().map(|q| q.value.cooked.as_deref().unwrap_or(q.value.raw.as_str()));
            first.map_or(true, is_exempt_href)
        }
        Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => {
            if id.name == "undefined" {
                return true;
            }
            let reference = semantic.scoping().get_reference(id.reference_id());
            let Some(sid) = reference.symbol_id() else {
                eprintln!("DEBUG is_safe_nav_arg: {} no symbol_id", id.name);
                return false
            };
            if !seen.insert(sid) {
                return false; // recursion guard
            }
            // Find the symbol's initializer.
            let decl_node_id = semantic.scoping().symbol_declaration(sid);
            let init = std::iter::once(decl_node_id)
                .chain(semantic.nodes().ancestor_ids(decl_node_id))
                .find_map(|aid| match semantic.nodes().kind(aid) {
                    AstKind::VariableDeclarator(vd) => vd.init.as_ref(),
                    _ => None,
                });
            match init {
                Some(init_expr) => is_safe_nav_arg(init_expr, resolve_locals, semantic, seen),
                None => false,
            }
        }
        _ => false,
    }
}

/// Parse a template-expression text and classify it as safe or not.
fn is_safe_template_expr<'a>(
    expr_text: &str,
    resolve_locals: &[String],
    instance_sem: Option<&'a Semantic<'a>>,
) -> bool {
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, expr_text, SourceType::mjs()).parse_expression();
    let Ok(expr) = parsed else {
        // Fallback: lenient — don't flag if we can't parse.
        return true;
    };
    let mut seen = FxHashSet::default();
    is_safe_template_root(&expr, resolve_locals, instance_sem, &mut seen)
}

/// Top-level safety check for a template-attribute expression. Differs from
/// `is_safe_nav_arg` only slightly: for Identifier refs, we look up the
/// declaration in the instance script's semantic model.
fn is_safe_template_root<'a>(
    expr: &Expression<'_>,
    resolve_locals: &[String],
    instance_sem: Option<&'a Semantic<'a>>,
    seen: &mut FxHashSet<String>,
) -> bool {
    match expr {
        Expression::CallExpression(_) => is_resolve_call(expr, resolve_locals),
        Expression::StringLiteral(l) => is_exempt_href(l.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            let first = t.quasis.first().map(|q| q.value.cooked.as_deref().unwrap_or(q.value.raw.as_str()));
            first.map_or(true, is_exempt_href)
        }
        Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => {
            if id.name == "undefined" {
                return true;
            }
            // The parsed expression's Identifier doesn't have a reference_id
            // resolved against our instance semantic. Resolve by NAME in root scope.
            let name = id.name.as_str();
            if !seen.insert(name.to_string()) {
                return false;
            }
            let Some(sem) = instance_sem else { return false };
            let scoping = sem.scoping();
            let Some(sid) = scoping.find_binding(scoping.root_scope_id(), Ident::new_const(name)) else {
                return false;
            };
            let decl_node_id = scoping.symbol_declaration(sid);
            let init = std::iter::once(decl_node_id)
                .chain(sem.nodes().ancestor_ids(decl_node_id))
                .find_map(|aid| match sem.nodes().kind(aid) {
                    AstKind::VariableDeclarator(vd) => vd.init.as_ref(),
                    _ => None,
                });
            match init {
                Some(init_expr) => is_safe_instance_expr(init_expr, resolve_locals, sem, seen),
                None => false,
            }
        }
        // `{foo ?? '/bar'}`, etc. — conservative: flag.
        _ => false,
    }
}

/// Safety check for an expression in the instance script (uses instance semantic
/// for reference resolution).
fn is_safe_instance_expr<'a>(
    expr: &'a Expression<'a>,
    resolve_locals: &[String],
    sem: &'a Semantic<'a>,
    seen: &mut FxHashSet<String>,
) -> bool {
    match expr {
        Expression::CallExpression(_) => is_resolve_call(expr, resolve_locals),
        Expression::StringLiteral(l) => is_exempt_href(l.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            let first = t.quasis.first().map(|q| q.value.cooked.as_deref().unwrap_or(q.value.raw.as_str()));
            first.map_or(true, is_exempt_href)
        }
        Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => {
            if id.name == "undefined" {
                return true;
            }
            let name = id.name.as_str();
            if !seen.insert(name.to_string()) {
                return false;
            }
            let reference = sem.scoping().get_reference(id.reference_id());
            let Some(sid) = reference.symbol_id() else { return false };
            let decl_node_id = sem.scoping().symbol_declaration(sid);
            let init = std::iter::once(decl_node_id)
                .chain(sem.nodes().ancestor_ids(decl_node_id))
                .find_map(|aid| match sem.nodes().kind(aid) {
                    AstKind::VariableDeclarator(vd) => vd.init.as_ref(),
                    _ => None,
                });
            match init {
                Some(init_expr) => is_safe_instance_expr(init_expr, resolve_locals, sem, seen),
                None => false,
            }
        }
        _ => false,
    }
}
