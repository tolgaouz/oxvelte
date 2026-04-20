//! `svelte/no-top-level-browser-globals` — disallow top-level access to browser globals.
//!
//! Uses oxc's semantic model to walk `IdentifierReference` nodes for known
//! browser globals. Each reference is then classified against four kinds of
//! skip signal:
//!
//! - **Scope**: if the ref is inside a function / arrow body, it's not
//!   top-level (vendor's `isTopLevelLocation`). Skipped unless a browser
//!   exit-guard (`if (!browser) return;`) upstream still applies.
//! - **Type annotation**: refs inside a `TSTypeAnnotation` are type-level
//!   and skipped.
//! - **`typeof` suppression**: `typeof window` on its own never writes the
//!   global; skipped when the reference is the argument of a `typeof` unary.
//! - **Guards**: the ref sits inside a conditional branch whose test
//!   evaluates the browser environment. Guard sources tracked:
//!     - `browser` / `BROWSER` identifier or namespace member from
//!       `$app/environment` / `esm-env` imports.
//!     - `globalThis.<name>` as a truthy check.
//!     - `typeof <name> !== 'undefined'` / `=== 'object'` and negations.
//!     - `import.meta.env.SSR` (negated: its alternate branch is the
//!       browser branch).
//!     - Previous-sibling `if (!browser) return;` / `if (browser) return;`
//!       patterns that make everything after them implicitly guarded.
//!
//! The template walk (mustache tags / `{#if}` branches) still keeps a
//! lightweight textual classifier for the test expression because Svelte
//! template expressions aren't pre-parsed into oxc AST here.

use crate::linter::{LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::ast::ast::{
    Declaration, Expression, ImportDeclarationSpecifier, ModuleExportName, Statement, Argument,
    IdentifierReference,
};
use oxc::ast::AstKind;
use oxc::semantic::{AstNodes, NodeId, Semantic};
use oxc::span::{GetSpan, Span};
use oxc::syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};
use std::collections::HashSet;

const BROWSER_GLOBALS: &[&str] = &[
    "window", "document", "navigator", "localStorage", "sessionStorage",
    "location", "history", "alert", "confirm", "prompt", "fetch",
    "XMLHttpRequest", "requestAnimationFrame", "cancelAnimationFrame",
    "setTimeout", "setInterval", "clearTimeout", "clearInterval",
    "customElements", "getComputedStyle", "matchMedia",
    "IntersectionObserver", "MutationObserver", "ResizeObserver",
];

pub struct NoTopLevelBrowserGlobals;

/// What a truthy-evaluation of a guard test protects: either every browser
/// global ("all"), or a single specifically-tested global name.
#[derive(Clone, Debug)]
enum Protect {
    All,
    Name(String),
}

fn protects_global(list: &[Protect], global: &str) -> bool {
    list.iter().any(|p| match p {
        Protect::All => true,
        Protect::Name(n) => n == global,
    })
}

/// Guard sources collected from the instance script's imports.
#[derive(Default, Debug)]
struct GuardCtx {
    /// Local names of `browser` / `BROWSER` imports from
    /// `$app/environment` / `esm-env` (including `import { browser as b }`
    /// renames).
    env_names: HashSet<String>,
    /// Local names of namespace imports from the same modules, so
    /// `env.browser` / `env.BROWSER` is treated as a guard too.
    env_namespaces: HashSet<String>,
}

fn collect_guard_ctx(sem: &Semantic<'_>) -> GuardCtx {
    let mut ctx = GuardCtx::default();
    for stmt in &sem.nodes().program().body {
        let Statement::ImportDeclaration(imp) = stmt else { continue };
        let src = imp.source.value.as_str();
        let is_app = src == "$app/environment";
        let is_esm = src == "esm-env";
        if !is_app && !is_esm { continue; }
        let Some(specs) = &imp.specifiers else { continue };
        for spec in specs {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let imported = match &s.imported {
                        ModuleExportName::IdentifierName(n) => n.name.as_str(),
                        ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                        ModuleExportName::StringLiteral(l) => l.value.as_str(),
                    };
                    if (is_app && imported == "browser") || (is_esm && imported == "BROWSER") {
                        ctx.env_names.insert(s.local.name.as_str().to_string());
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    ctx.env_namespaces.insert(s.local.name.as_str().to_string());
                }
                _ => {}
            }
        }
    }
    ctx
}

/// Returns `(protect_when_truthy, protect_when_falsy)` for a test expression.
fn analyze_test(expr: &Expression<'_>, ctx: &GuardCtx) -> (Vec<Protect>, Vec<Protect>) {
    match expr {
        Expression::ParenthesizedExpression(p) => analyze_test(&p.expression, ctx),
        Expression::Identifier(id) => {
            if ctx.env_names.contains(id.name.as_str()) {
                (vec![Protect::All], vec![])
            } else { (vec![], vec![]) }
        }
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
            let (p, n) = analyze_test(&u.argument, ctx);
            (n, p)
        }
        Expression::StaticMemberExpression(m) => analyze_member_test(&m.object, m.property.name.as_str(), ctx),
        Expression::ComputedMemberExpression(_) => (vec![], vec![]),
        Expression::ChainExpression(c) => match &c.expression {
            oxc::ast::ast::ChainElement::StaticMemberExpression(m) =>
                analyze_member_test(&m.object, m.property.name.as_str(), ctx),
            _ => (vec![], vec![]),
        },
        Expression::BinaryExpression(b) => analyze_binary_test(b, ctx),
        Expression::LogicalExpression(le) => {
            // Only conservative rules; AND combines positives, OR combines negatives.
            match le.operator {
                LogicalOperator::And => {
                    let (lp, _) = analyze_test(&le.left, ctx);
                    let (rp, _) = analyze_test(&le.right, ctx);
                    let mut out = lp;
                    out.extend(rp);
                    (out, vec![])
                }
                LogicalOperator::Or => {
                    let (_, ln) = analyze_test(&le.left, ctx);
                    let (_, rn) = analyze_test(&le.right, ctx);
                    let mut out = ln;
                    out.extend(rn);
                    (vec![], out)
                }
                _ => (vec![], vec![]),
            }
        }
        _ => (vec![], vec![]),
    }
}

fn analyze_member_test(
    object: &Expression<'_>,
    prop: &str,
    ctx: &GuardCtx,
) -> (Vec<Protect>, Vec<Protect>) {
    // `globalThis.<browserGlobal>` → protects that specific name.
    // `globalThis.window` and `globalThis.document` are universal browser
    // checks (vendor's `browserEnvironment: true` for those names).
    if let Expression::Identifier(obj) = object {
        if obj.name == "globalThis" && BROWSER_GLOBALS.contains(&prop) {
            let p = if prop == "window" || prop == "document" {
                Protect::All
            } else {
                Protect::Name(prop.to_string())
            };
            return (vec![p], vec![]);
        }
        // `env.browser` / `env.BROWSER` where env is a namespace import.
        if ctx.env_namespaces.contains(obj.name.as_str())
            && (prop == "browser" || prop == "BROWSER")
        {
            return (vec![Protect::All], vec![]);
        }
    }
    // `import.meta.env.SSR` → its truthy branch is server; alternate is browser.
    if prop == "SSR" {
        if let Expression::StaticMemberExpression(m) = object {
            if m.property.name == "env" {
                if let Expression::MetaProperty(mp) = &m.object {
                    if mp.meta.name == "import" && mp.property.name == "meta" {
                        return (vec![], vec![Protect::All]);
                    }
                }
            }
        }
    }
    (vec![], vec![])
}

fn analyze_binary_test(
    b: &oxc::ast::ast::BinaryExpression<'_>,
    ctx: &GuardCtx,
) -> (Vec<Protect>, Vec<Protect>) {
    // typeof X (!)=== 'undefined' | 'object'
    let (typeof_arg, other) = match (&b.left, &b.right) {
        (Expression::UnaryExpression(u), _) if u.operator == UnaryOperator::Typeof =>
            (&u.argument, &b.right),
        (_, Expression::UnaryExpression(u)) if u.operator == UnaryOperator::Typeof =>
            (&u.argument, &b.left),
        _ => return analyze_binary_non_typeof(b, ctx),
    };
    let Expression::Identifier(id) = typeof_arg else { return (vec![], vec![]) };
    let name = id.name.as_str();
    let Expression::StringLiteral(sl) = other else { return (vec![], vec![]) };
    let val = sl.value.as_str();
    let positive_when_truthy = match (&b.operator, val) {
        (BinaryOperator::Inequality, "undefined") | (BinaryOperator::StrictInequality, "undefined") => true,
        (BinaryOperator::Equality, "undefined") | (BinaryOperator::StrictEquality, "undefined") => false,
        (BinaryOperator::Inequality, "object") | (BinaryOperator::StrictInequality, "object") => false,
        (BinaryOperator::Equality, "object") | (BinaryOperator::StrictEquality, "object") => true,
        _ => return (vec![], vec![]),
    };
    // Vendor: `typeof window` / `typeof document` are universal browser
    // checks — if those are defined, the whole browser environment is
    // assumed. Any other name protects only itself.
    let protect = if name == "window" || name == "document" {
        Protect::All
    } else {
        Protect::Name(name.to_string())
    };
    if positive_when_truthy {
        (vec![protect], vec![])
    } else {
        (vec![], vec![protect])
    }
}

fn analyze_binary_non_typeof(
    b: &oxc::ast::ast::BinaryExpression<'_>,
    _ctx: &GuardCtx,
) -> (Vec<Protect>, Vec<Protect>) {
    // `globalThis.<name> instanceof <anything>` — vendor treats this as a
    // truthy browser check on `<name>`. Left-side must be the member;
    // right-side unconstrained.
    if b.operator == BinaryOperator::Instanceof {
        if let Expression::StaticMemberExpression(m) = &b.left {
            if let Expression::Identifier(obj) = &m.object {
                let prop = m.property.name.as_str();
                if obj.name == "globalThis" && BROWSER_GLOBALS.contains(&prop) {
                    let protect = if prop == "window" || prop == "document" {
                        Protect::All
                    } else {
                        Protect::Name(prop.to_string())
                    };
                    return (vec![protect], vec![]);
                }
            }
        }
    }

    // `globalThis.<name> (!)=== undefined/null`
    let (member_side, other_side) = match (&b.left, &b.right) {
        (Expression::StaticMemberExpression(_), _) => (&b.left, &b.right),
        (_, Expression::StaticMemberExpression(_)) => (&b.right, &b.left),
        _ => return (vec![], vec![]),
    };
    let Expression::StaticMemberExpression(m) = member_side else { return (vec![], vec![]) };
    let Expression::Identifier(obj) = &m.object else { return (vec![], vec![]) };
    if obj.name != "globalThis" { return (vec![], vec![]) }
    let prop = m.property.name.as_str();
    if !BROWSER_GLOBALS.contains(&prop) { return (vec![], vec![]) }

    // Match literal `undefined` identifier or `null` literal.
    let other_kind = match other_side {
        Expression::Identifier(id) if id.name == "undefined" => "undefined",
        Expression::NullLiteral(_) => "null",
        _ => return (vec![], vec![]),
    };
    // `globalThis.x !== undefined` / `=== undefined` / `!= null` / `== null`.
    let positive_when_truthy = match (&b.operator, other_kind) {
        (BinaryOperator::Inequality | BinaryOperator::StrictInequality, "undefined") => true,
        (BinaryOperator::Equality | BinaryOperator::StrictEquality, "undefined") => false,
        // `!= null` matches both null AND undefined → truthy means defined.
        // `!==  null` only excludes null, not undefined — can't conclude.
        (BinaryOperator::Inequality, "null") => true,
        (BinaryOperator::Equality, "null") => false,
        _ => return (vec![], vec![]),
    };
    let protect = if prop == "window" || prop == "document" {
        Protect::All
    } else {
        Protect::Name(prop.to_string())
    };
    if positive_when_truthy {
        (vec![protect], vec![])
    } else {
        (vec![], vec![protect])
    }
}

/// Walk the ancestor chain of `node_id`. Return true if any enclosing
/// control-flow branch protects `global`.
fn is_inside_guard(
    nodes: &AstNodes<'_>,
    node_id: NodeId,
    ctx: &GuardCtx,
    global: &str,
) -> bool {
    let ref_span = nodes.kind(node_id).span();
    let mut id = node_id;
    loop {
        let parent = nodes.parent_id(id);
        if parent == id { return false; }
        let kind = nodes.kind(parent);
        match kind {
            AstKind::IfStatement(ifs) => {
                let (pos, neg) = analyze_test(&ifs.test, ctx);
                if span_contains(ifs.consequent.span(), ref_span) && protects_global(&pos, global) {
                    return true;
                }
                if let Some(alt) = &ifs.alternate {
                    if span_contains(alt.span(), ref_span) && protects_global(&neg, global) {
                        return true;
                    }
                }
            }
            AstKind::ConditionalExpression(ce) => {
                let (pos, neg) = analyze_test(&ce.test, ctx);
                if span_contains(ce.consequent.span(), ref_span) && protects_global(&pos, global) {
                    return true;
                }
                if span_contains(ce.alternate.span(), ref_span) && protects_global(&neg, global) {
                    return true;
                }
            }
            AstKind::LogicalExpression(le) => {
                if span_contains(le.right.span(), ref_span) {
                    match le.operator {
                        LogicalOperator::And => {
                            let (pos, _) = analyze_test(&le.left, ctx);
                            if protects_global(&pos, global) { return true; }
                        }
                        LogicalOperator::Or => {
                            let (_, neg) = analyze_test(&le.left, ctx);
                            if protects_global(&neg, global) { return true; }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        id = parent;
    }
}

/// Walk the block statements preceding `node_id` inside its enclosing
/// `BlockStatement` / `Program`. If a preceding `IfStatement`'s consequent
/// jumps on every path and its test is analyzable, reaching this statement
/// means the falsy branch's protections apply.
fn after_exit_guard(
    nodes: &AstNodes<'_>,
    node_id: NodeId,
    ctx: &GuardCtx,
    global: &str,
) -> bool {
    // Walk up to find the enclosing Statement whose parent is a
    // BlockStatement / Program. Then scan preceding sibling statements.
    let mut id = node_id;
    loop {
        let parent = nodes.parent_id(id);
        if parent == id { return false; }
        let parent_kind = nodes.kind(parent);
        let siblings: &[Statement] = match parent_kind {
            AstKind::BlockStatement(b) => b.body.as_slice(),
            AstKind::Program(p) => p.body.as_slice(),
            _ => { id = parent; continue; }
        };
        // Find the Statement in `siblings` that contains `id`.
        let id_span = nodes.kind(id).span();
        let mut prev_iter = Vec::<&Statement>::new();
        for stmt in siblings {
            if span_contains(stmt.span(), id_span) { break; }
            prev_iter.push(stmt);
        }
        for stmt in prev_iter.iter().rev() {
            let Statement::IfStatement(ifs) = stmt else { continue };
            // Only a guard if consequent jumps unconditionally AND no alternate
            // (if there's an alternate, control flow isn't "skip-past-if").
            if ifs.alternate.is_some() { continue; }
            if !has_jump_in_all_paths(&ifs.consequent) { continue; }
            let (_, neg) = analyze_test(&ifs.test, ctx);
            if protects_global(&neg, global) { return true; }
        }
        return false;
    }
}

fn has_jump_in_all_paths(stmt: &Statement<'_>) -> bool {
    if is_jump_statement(stmt) { return true; }
    match stmt {
        Statement::BlockStatement(b) => b.body.iter().any(has_jump_in_all_paths),
        Statement::IfStatement(ifs) => {
            ifs.alternate.as_ref()
                .is_some_and(|a| has_jump_in_all_paths(a))
                && has_jump_in_all_paths(&ifs.consequent)
        }
        _ => false,
    }
}

fn is_jump_statement(stmt: &Statement<'_>) -> bool {
    matches!(stmt,
        Statement::ReturnStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ThrowStatement(_)
    )
}

fn span_contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Is `ref_id` the direct argument of a `typeof` UnaryExpression?
/// (`typeof window` should never fire.)
fn is_typeof_argument(nodes: &AstNodes<'_>, ref_id: NodeId) -> bool {
    let parent = nodes.parent_id(ref_id);
    matches!(nodes.kind(parent), AstKind::UnaryExpression(u) if u.operator == UnaryOperator::Typeof)
}

/// Is `ref_id` the object side of a `globalThis.<name>` StaticMemberExpression?
/// We don't flag these because the vendor treats `globalThis.window` as a
/// guarded access (and `globalThis.window` as a *value* read is separately
/// tracked as a reference to `window`).
fn is_globalthis_of(nodes: &AstNodes<'_>, ref_id: NodeId, name: &str) -> bool {
    let _ = name;
    let parent = nodes.parent_id(ref_id);
    let AstKind::StaticMemberExpression(m) = nodes.kind(parent) else { return false };
    let Expression::Identifier(obj) = &m.object else { return false };
    obj.name == "globalThis"
}

/// Collect spans of every function / arrow expression in the program —
/// references inside are not top-level.
fn collect_function_spans(sem: &Semantic<'_>) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for node in sem.nodes().iter() {
        let span = match node.kind() {
            AstKind::Function(f) => f.span,
            AstKind::ArrowFunctionExpression(a) => a.span,
            _ => continue,
        };
        out.push((span.start, span.end));
    }
    out
}

/// Collect spans of every `TSTypeAnnotation` node — references inside are
/// type-level and ignored.
fn collect_type_annotation_spans(sem: &Semantic<'_>) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for node in sem.nodes().iter() {
        if let AstKind::TSTypeAnnotation(t) = node.kind() {
            out.push((t.span.start, t.span.end));
        }
    }
    out
}

fn is_inside_any(span: Span, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|(s, e)| *s <= span.start && span.end <= *e)
}

impl Rule for NoTopLevelBrowserGlobals {
    fn name(&self) -> &'static str {
        "svelte/no-top-level-browser-globals"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = &ctx.ast.instance else { return };
        if script.module { return; }
        let Some(sem) = ctx.instance_semantic else {
            check_template_nodes(&ctx.ast.html.nodes, ctx, false);
            return;
        };
        let scoping = sem.scoping();
        let nodes = sem.nodes();

        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        let guard_ctx = collect_guard_ctx(sem);
        let function_spans = collect_function_spans(sem);
        let type_annotation_spans = collect_type_annotation_spans(sem);

        for node in nodes.iter() {
            let AstKind::IdentifierReference(id) = node.kind() else { continue };
            let name = id.name.as_str();
            let Some(&global) = BROWSER_GLOBALS.iter().find(|g| **g == name) else { continue };
            // Locally-shadowed reference (`let window = ...`) → skip.
            if scoping.get_reference(id.reference_id()).symbol_id().is_some() { continue; }
            // Type-level use: `let x: Window` → skip.
            if is_inside_any(id.span, &type_annotation_spans) { continue; }
            // `typeof window` alone — never a read, skip.
            if is_typeof_argument(nodes, node.id()) { continue; }
            // `globalThis.window` — the identifier is `globalThis`, not `window`,
            // so we'd only get here if someone wrote `window` as the property of
            // `globalThis`... which oxc models as IdentifierName, not
            // IdentifierReference. This branch is defensive.
            if is_globalthis_of(nodes, node.id(), global) { continue; }

            // Top-level check: inside any function / arrow expression → not
            // top-level. Inside-function references can still be browser-
            // unsafe if an earlier exit-guard applies, but for now the rule
            // scope matches vendor (only top-level).
            if is_inside_any(id.span, &function_spans) { continue; }

            // Ancestor guard: `if (browser) { ref }`, `browser ? ref : x`,
            // `browser && ref`, `typeof x !== 'undefined' && ref.href`, etc.
            if is_inside_guard(nodes, node.id(), &guard_ctx, global) { continue; }

            // Preceding `if (!browser) return;` guard-exit in the enclosing block.
            if after_exit_guard(nodes, node.id(), &guard_ctx, global) { continue; }

            // `globalThis.location?.href` — optional chain through the
            // globalThis access is treated as guarded (it's nullish-safe).
            if is_in_optional_chain_of_globalthis(nodes, node.id(), global) { continue; }

            let byte_offset = id.span.start as usize;
            let s = (content_offset + byte_offset) as u32;
            ctx.diagnostic(
                format!("Unexpected top-level browser global variable \"{}\".", global),
                Span::new(s, s + global.len() as u32),
            );
        }

        check_template_nodes(&ctx.ast.html.nodes, ctx, false);
    }
}

/// True iff `ref_id` is a descendant of an optional-chain access through
/// `globalThis.<global>`. E.g. in `globalThis.location?.href`, the
/// `globalThis.location` read is guarded.
fn is_in_optional_chain_of_globalthis(
    nodes: &AstNodes<'_>,
    ref_id: NodeId,
    _global: &str,
) -> bool {
    // Walk up a few levels: IdentifierReference → StaticMemberExpression (globalThis.X)
    // → ChainExpression? optional access makes this whole chain guarded.
    let parent = nodes.parent_id(ref_id);
    let AstKind::StaticMemberExpression(m) = nodes.kind(parent) else { return false };
    let Expression::Identifier(obj) = &m.object else { return false };
    if obj.name != "globalThis" { return false; }
    let grand = nodes.parent_id(parent);
    match nodes.kind(grand) {
        AstKind::ChainExpression(_) => true,
        AstKind::StaticMemberExpression(outer) => {
            // Check if outer member is optional access through this.
            outer.optional
        }
        _ => false,
    }
}

fn check_template_nodes(nodes: &[TemplateNode], ctx: &mut LintContext<'_>, in_browser_ctx: bool) {
    for node in nodes {
        match node {
            TemplateNode::MustacheTag(tag) => {
                if !in_browser_ctx {
                    check_expr_for_globals(&tag.expression, tag.span, ctx);
                }
            }
            TemplateNode::IfBlock(block) => {
                let cond = block.test.trim();

                let is_browser_guard = cond.contains("browser") || cond.contains("BROWSER")
                    || cond.starts_with("typeof window") || cond.starts_with("typeof document")
                    || cond.starts_with("globalThis.");
                let is_negated = cond.starts_with('!');

                let cons_browser = in_browser_ctx || (is_browser_guard && !is_negated);
                let cons_server = is_browser_guard && is_negated;
                check_template_nodes(&block.consequent.nodes, ctx, cons_browser || (!cons_server && in_browser_ctx));

                if let Some(alt) = &block.alternate {
                    let alt_browser = in_browser_ctx || (is_browser_guard && is_negated);
                    if let TemplateNode::IfBlock(else_if) = alt.as_ref() {
                        let else_test = else_if.test.trim();
                        if else_test.is_empty() {
                            check_template_nodes(&else_if.consequent.nodes, ctx, alt_browser);
                        } else {
                            let ebc = else_test == "browser" || else_test == "BROWSER"
                                || else_test.contains("globalThis.");
                            let eng = else_test.starts_with('!');
                            let eb = alt_browser || (ebc && !eng);
                            check_template_nodes(&else_if.consequent.nodes, ctx, eb);
                            if let Some(a2) = &else_if.alternate {
                                let eb2 = alt_browser || (ebc && eng);
                                if let TemplateNode::IfBlock(a2if) = a2.as_ref() {
                                    check_template_nodes(&a2if.consequent.nodes, ctx, eb2);
                                }
                            }
                        }
                    }
                }
            }
            TemplateNode::Element(el) => {
                check_template_nodes(&el.children, ctx, in_browser_ctx);
            }
            TemplateNode::EachBlock(block) => {
                check_template_nodes(&block.body.nodes, ctx, in_browser_ctx);
                if let Some(fb) = &block.fallback {
                    check_template_nodes(&fb.nodes, ctx, in_browser_ctx);
                }
            }
            TemplateNode::KeyBlock(block) => {
                check_template_nodes(&block.body.nodes, ctx, in_browser_ctx);
            }
            TemplateNode::SnippetBlock(block) => {
                check_template_nodes(&block.body.nodes, ctx, in_browser_ctx);
            }
            _ => {}
        }
    }
}

fn is_word_boundary(text: &str, pos: usize, len: usize) -> bool {
    let bytes = text.as_bytes();
    (pos == 0 || !matches!(bytes[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'.'))
        && (pos + len >= bytes.len() || !matches!(bytes[pos + len], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'))
}

fn check_expr_for_globals(expr: &str, span: Span, ctx: &mut LintContext<'_>) {
    for global in BROWSER_GLOBALS {
        if let Some(pos) = expr.find(global) {
            if !is_word_boundary(expr, pos, global.len()) { continue; }
            ctx.diagnostic(format!("Unexpected top-level browser global variable \"{}\".", global), span);
        }
    }
}
