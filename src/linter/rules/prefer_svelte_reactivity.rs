//! `svelte/prefer-svelte-reactivity` — prefer Svelte reactive classes over mutable
//! built-in JS classes (`Date`, `Map`, `Set`, `URL`, `URLSearchParams`).
//! ⭐ Recommended.
//!
//! Vendor algorithm: walk every `new Builtin(...)` whose callee resolves to the
//! corresponding **global** binding, find the `VariableDeclarator` it
//! initializes, then look at the resulting symbol's references for mutating
//! method calls or assignments to mutable properties. In `.svelte.[js|ts]`
//! files, an export of any of those declarators is also reported. Vendor
//! relies on `@eslint-community/eslint-utils`'s `ReferenceTracker`, which
//! follows aliasing and renamed-import chains via the scope manager.
//!
//! Our equivalent uses `oxc_semantic`'s scope manager:
//! * "global" callee = the `Identifier`'s `reference_id` has no resolved
//!   binding (`scoping.has_binding` returns `false`) — same shape as vendor's
//!   `iterateGlobalReferences`.
//! * Aliasing through `const m2 = m;` (or assignment) is handled by walking
//!   the tracked symbol's resolved-references and queueing any symbol it
//!   initializes. We iterate to a fixed point so chains of any length work.
//! * Template mustaches: traverse `MustacheTag.expression_ast` (already
//!   typed in our template AST) and report direct `new Builtin().mutator(...)`
//!   chains, plus mutations on names that match a tracked binding from any
//!   script. The mustache AST has its own allocator, so we identify by name
//!   rather than `SymbolId`.

use crate::ast::{MustacheTag, TemplateNode};
use crate::linter::{walk_template_nodes, LintContext, Rule};
use oxc::ast::ast::{AssignmentTarget, BindingPattern, Expression};
use oxc::ast::AstKind;
use oxc::semantic::{AstNodes, NodeId, Scoping, Semantic, SymbolId};
use oxc::span::{GetSpan, Span};
use std::collections::{HashMap, HashSet};

pub struct PreferSvelteReactivity;

/// `mutating_methods` are detected as `obj.method(...)` (CallExpression on a
/// member access). `mutating_props` are detected as the LHS of an
/// `AssignmentExpression` — used by `URL` (vendor's `isURLMutable`).
struct BuiltinClass {
    name: &'static str,
    svelte_name: &'static str,
    mutating_methods: &'static [&'static str],
    mutating_props: &'static [&'static str],
}

#[rustfmt::skip]
const BUILTIN_CLASSES: &[BuiltinClass] = &[
    BuiltinClass {
        name: "Date", svelte_name: "SvelteDate",
        mutating_methods: &[
            "setDate", "setFullYear", "setHours", "setMilliseconds", "setMinutes",
            "setMonth", "setSeconds", "setTime", "setUTCDate", "setUTCFullYear",
            "setUTCHours", "setUTCMilliseconds", "setUTCMinutes", "setUTCMonth",
            "setUTCSeconds", "setYear",
        ],
        mutating_props: &[],
    },
    BuiltinClass { name: "Map", svelte_name: "SvelteMap",
        mutating_methods: &["clear", "delete", "set"], mutating_props: &[] },
    BuiltinClass { name: "Set", svelte_name: "SvelteSet",
        mutating_methods: &["add", "clear", "delete"], mutating_props: &[] },
    BuiltinClass { name: "URL", svelte_name: "SvelteURL",
        mutating_methods: &[],
        mutating_props: &[
            "hash", "host", "hostname", "href", "password",
            "pathname", "port", "protocol", "search", "username",
        ],
    },
    BuiltinClass { name: "URLSearchParams", svelte_name: "SvelteURLSearchParams",
        mutating_methods: &["append", "delete", "set", "sort"], mutating_props: &[] },
];

fn message(builtin: &BuiltinClass) -> String {
    format!(
        "Found a mutable instance of the built-in {} class. Use {} instead.",
        builtin.name, builtin.svelte_name
    )
}

fn lookup_builtin(name: &str) -> Option<&'static BuiltinClass> {
    BUILTIN_CLASSES.iter().find(|b| b.name == name)
}

impl Rule for PreferSvelteReactivity {
    fn name(&self) -> &'static str {
        "svelte/prefer-svelte-reactivity"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn applies_to_svelte_scripts(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let is_module = ctx.is_svelte_module;

        // Names of any tracked symbol (per builtin) across both scripts —
        // used by the template walker to spot `name.mutator(...)` patterns.
        let mut template_targets: HashMap<&'static str, HashSet<String>> = HashMap::new();
        for b in BUILTIN_CLASSES {
            template_targets.insert(b.name, HashSet::new());
        }

        for (sem, offset) in [
            (ctx.instance_semantic, ctx.instance_content_offset),
            (ctx.module_semantic, ctx.module_content_offset),
        ] {
            let Some(sem) = sem else { continue };
            check_script(ctx, sem, offset, is_module, &mut template_targets);
        }

        // Template: detect both `new Builtin().mutator(...)` chains directly
        // appearing in a mustache and bare `name.mutator(...)` patterns where
        // `name` is a tracked symbol from either script.
        if !is_module {
            check_templates(ctx, &template_targets);
        }
    }
}

// ---------------------------------------------------------------------------
// Script analysis
// ---------------------------------------------------------------------------

fn check_script<'a>(
    ctx: &mut LintContext<'a>,
    sem: &'a Semantic<'a>,
    content_offset: u32,
    is_module: bool,
    template_targets: &mut HashMap<&'static str, HashSet<String>>,
) {
    let scoping = sem.scoping();
    let nodes = sem.nodes();

    // Pass 1: find every `new Builtin(...)` whose callee is the global
    // builtin. Group them by builtin name.
    let mut constructions: Vec<Construction> = Vec::new();
    for node in nodes.iter() {
        let AstKind::NewExpression(ne) = node.kind() else {
            continue;
        };
        let Expression::Identifier(callee) = &ne.callee else {
            continue;
        };
        let name = callee.name.as_str();
        let Some(builtin) = lookup_builtin(name) else {
            continue;
        };
        // `scoping.has_binding(reference_id)` returns true if the identifier
        // resolves to a local binding (import, let, function, etc.).
        // Vendor's `iterateGlobalReferences` skips those entirely.
        if scoping.has_binding(callee.reference_id()) {
            continue;
        }
        constructions.push(Construction {
            builtin,
            new_expr_span: ne.span,
            new_expr_node_id: node.id(),
        });
    }
    if constructions.is_empty() {
        return;
    }

    // Pass 2: collect symbols that are re-exported via specifier
    // (`export { a, b as default }`). For exports that include their
    // declaration directly (`export const x = ...`, `export default x`),
    // we walk ancestors at construction-detection time instead.
    let exported_specifier_symbols: HashSet<SymbolId> = if is_module {
        collect_exported_specifier_symbols(sem)
    } else {
        HashSet::new()
    };

    // Pass 3: for each construction find the symbol it initializes, then
    // expand to the set of aliased symbols. A construction may not bind to
    // any symbol (e.g. `new Date(); foo()`, `export default new Date()`,
    // `array.push(new Date())`).
    //
    // `tracked` maps a tracked symbol → (builtin, origin construction span).
    let mut tracked: HashMap<SymbolId, (&'static BuiltinClass, Span)> = HashMap::new();
    // Set of construction spans we'll report — keyed on (start, end) for dedup.
    let mut to_report: HashSet<(u32, u32, &'static str)> = HashSet::new();

    for ctor in &constructions {
        // Module-export check (vendor's `isIn(node, exportedVar)`):
        //   * `export const x = new Date()` — the construction's ancestor
        //     chain hits an `ExportNamedDeclaration` directly.
        //   * `export default new Date()` — ditto for `ExportDefaultDeclaration`.
        //   * `export default x;` / `export { x }` — the construction binds
        //     to `x` (a tracked symbol below); we mark `x` as exported and
        //     report after we know the construction's symbol.
        if is_module && is_inside_export_decl(nodes, ctor.new_expr_node_id) {
            to_report.insert((
                ctor.new_expr_span.start,
                ctor.new_expr_span.end,
                ctor.builtin.name,
            ));
            continue;
        }

        // Direct mutation: `new Date().setHours(...)` or
        // `(new URL(...)).href = "..."`.
        if is_directly_mutated(nodes, ctor.new_expr_node_id, ctor.builtin) {
            to_report.insert((
                ctor.new_expr_span.start,
                ctor.new_expr_span.end,
                ctor.builtin.name,
            ));
            continue;
        }

        // Otherwise, locate the binding the construction flows into.
        if let Some(sym) = target_symbol(nodes, scoping, ctor.new_expr_node_id) {
            tracked
                .entry(sym)
                .or_insert((ctor.builtin, ctor.new_expr_span));
        }
    }

    // Specifier-style export: `const v = new Date(); export { v };` →
    // anything tracked + in `exported_specifier_symbols` reports.
    if is_module {
        for (sid, &(builtin, origin_span)) in &tracked {
            if exported_specifier_symbols.contains(sid) {
                to_report.insert((origin_span.start, origin_span.end, builtin.name));
            }
        }
    }

    // Aliasing closure: `const m2 = m;` where `m` is tracked → also track `m2`.
    expand_aliases(scoping, nodes, &mut tracked);

    // Pass 4: detect mutations on every tracked symbol.
    for (&sid, &(builtin, origin_span)) in &tracked {
        if symbol_is_mutated(scoping, nodes, sid, builtin) {
            to_report.insert((origin_span.start, origin_span.end, builtin.name));
        }
    }

    // Pass 5: feed tracked names into the template-side detector.
    for (&sid, &(builtin, _)) in &tracked {
        let name = scoping.symbol_name(sid).to_string();
        template_targets
            .get_mut(builtin.name)
            .expect("builtin entry seeded")
            .insert(name);
    }

    // Emit, in source order.
    let mut sorted: Vec<_> = to_report.into_iter().collect();
    sorted.sort_by_key(|(s, e, _)| (*s, *e));
    for (start, end, name) in sorted {
        let builtin = lookup_builtin(name).expect("seeded");
        let abs = Span::new(content_offset + start, content_offset + end);
        ctx.diagnostic(message(builtin), abs);
    }
}

struct Construction {
    builtin: &'static BuiltinClass,
    new_expr_span: Span,
    new_expr_node_id: NodeId,
}

/// True if any ancestor of `node_id` is an `ExportNamedDeclaration` or
/// `ExportDefaultDeclaration`. Mirrors vendor's `isIn(node, exportedVar)`
/// for the inline-declaration cases (`export const x = ...`,
/// `export default new Foo()`).
fn is_inside_export_decl(nodes: &AstNodes, node_id: NodeId) -> bool {
    for ancestor in nodes.ancestor_ids(node_id) {
        match nodes.kind(ancestor) {
            AstKind::ExportNamedDeclaration(_) | AstKind::ExportDefaultDeclaration(_) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Collect symbols referenced by *specifier-only* exports
/// (`export { a, b as default }`) — i.e. exports whose declaration AST
/// is an `ExportNamedDeclaration` with `declaration: None`. Inline
/// declarations are handled by `is_inside_export_decl`.
fn collect_exported_specifier_symbols<'a>(sem: &'a Semantic<'a>) -> HashSet<SymbolId> {
    let scoping = sem.scoping();
    let nodes = sem.nodes();
    let mut out = HashSet::new();
    for node in nodes.iter() {
        let AstKind::ExportNamedDeclaration(end) = node.kind() else {
            continue;
        };
        if end.declaration.is_some() {
            continue;
        }
        for spec in &end.specifiers {
            // `spec.local` is a `ModuleExportName`. For specifier-only
            // exports it's almost always an `IdentifierReference` whose
            // `reference_id` resolves to a local binding.
            if let oxc::ast::ast::ModuleExportName::IdentifierReference(ident) = &spec.local {
                if let Some(rid) = ident.reference_id.get() {
                    if let Some(sid) = scoping.get_reference(rid).symbol_id() {
                        out.insert(sid);
                    }
                }
            }
        }
    }
    out
}

/// True if the `new Builtin(...)` expression itself is the receiver of a
/// directly mutating method/property, e.g. `new Map().set(1,2)` or
/// `(new URL(s)).href = "/"`. We climb past `( )`, `cond ? a : b` (when
/// the `new` is one branch), `a || b`, etc., matching vendor's reach.
fn is_directly_mutated(nodes: &AstNodes, new_expr_id: NodeId, builtin: &BuiltinClass) -> bool {
    let mut current = new_expr_id;
    loop {
        let parent = nodes.parent_id(current);
        match nodes.kind(parent) {
            AstKind::ParenthesizedExpression(_)
            | AstKind::LogicalExpression(_)
            | AstKind::ConditionalExpression(_)
            | AstKind::SequenceExpression(_) => current = parent,
            _ => break,
        }
    }
    let parent = nodes.parent_id(current);
    let AstKind::StaticMemberExpression(member) = nodes.kind(parent) else {
        return false;
    };
    let prop = member.property.name.as_str();
    let gp = nodes.parent_id(parent);
    if builtin.mutating_methods.contains(&prop) {
        if let AstKind::CallExpression(ce) = nodes.kind(gp) {
            // `obj.method(...)` — confirm member is the callee, not an arg.
            if ce.callee.span() == member.span {
                return true;
            }
        }
    }
    if builtin.mutating_props.contains(&prop) {
        if let AstKind::AssignmentExpression(ae) = nodes.kind(gp) {
            if ae.left.span() == member.span {
                return true;
            }
        }
    }
    false
}

/// Walk from a `new Builtin(...)` up through `( )`, `cond ? a : b`,
/// `a ?? b`, `(a, b)` etc. until we hit a `VariableDeclarator` or
/// `AssignmentExpression`. Returns the bound symbol if one exists.
fn target_symbol(nodes: &AstNodes, scoping: &Scoping, new_expr_id: NodeId) -> Option<SymbolId> {
    for ancestor in nodes.ancestor_ids(new_expr_id) {
        match nodes.kind(ancestor) {
            AstKind::VariableDeclarator(decl) => {
                return binding_symbol_from_pattern(&decl.id);
            }
            AstKind::AssignmentExpression(assign) => {
                let AssignmentTarget::AssignmentTargetIdentifier(ident) = &assign.left else {
                    return None;
                };
                return ident
                    .reference_id
                    .get()
                    .and_then(|r| scoping.get_reference(r).symbol_id());
            }
            AstKind::ParenthesizedExpression(_)
            | AstKind::LogicalExpression(_)
            | AstKind::ConditionalExpression(_)
            | AstKind::SequenceExpression(_)
            | AstKind::TSAsExpression(_)
            | AstKind::TSSatisfiesExpression(_)
            | AstKind::TSNonNullExpression(_) => continue,
            _ => return None,
        }
    }
    None
}

fn binding_symbol_from_pattern(pat: &BindingPattern<'_>) -> Option<SymbolId> {
    match pat {
        BindingPattern::BindingIdentifier(id) => id.symbol_id.get(),
        // Destructuring patterns don't have a single target — vendor's
        // `ReferenceTracker` follows them member-by-member, but in practice
        // none of the fixtures destructure a `new Map()`. We bail rather
        // than guessing.
        _ => None,
    }
}

/// Given an initial set of tracked symbols, expand it to include every
/// symbol that is initialized from a tracked symbol's value.
/// `const m = new Map(); const m2 = m; const m3 = m2;` → all three tracked.
///
/// We iterate to a fixed point so chains of any length resolve.
fn expand_aliases(
    scoping: &Scoping,
    nodes: &AstNodes,
    tracked: &mut HashMap<SymbolId, (&'static BuiltinClass, Span)>,
) {
    loop {
        let mut added: Vec<(SymbolId, &'static BuiltinClass, Span)> = Vec::new();
        for (&sid, &(builtin, origin)) in tracked.iter() {
            for reference in scoping.get_resolved_references(sid) {
                if !reference.is_read() {
                    continue;
                }
                let ref_node = reference.node_id();
                // `let x = m;`, `x = m;`, `const x = (m);` — walk the same
                // wrappers `target_symbol` does.
                if let Some(other) = target_symbol(nodes, scoping, ref_node) {
                    if !tracked.contains_key(&other) {
                        added.push((other, builtin, origin));
                    }
                }
            }
        }
        if added.is_empty() {
            break;
        }
        for (s, b, origin) in added {
            tracked.entry(s).or_insert((b, origin));
        }
    }
}

/// True if any reference of `sid` reads into a mutating member access.
fn symbol_is_mutated(
    scoping: &Scoping,
    nodes: &AstNodes,
    sid: SymbolId,
    builtin: &BuiltinClass,
) -> bool {
    for reference in scoping.get_resolved_references(sid) {
        if !reference.is_read() {
            continue;
        }
        let parent = nodes.parent_id(reference.node_id());
        let AstKind::StaticMemberExpression(member) = nodes.kind(parent) else {
            continue;
        };
        let prop = member.property.name.as_str();
        let gp = nodes.parent_id(parent);
        if builtin.mutating_methods.contains(&prop) {
            if let AstKind::CallExpression(ce) = nodes.kind(gp) {
                if ce.callee.span() == member.span {
                    return true;
                }
            }
        }
        if builtin.mutating_props.contains(&prop) {
            if let AstKind::AssignmentExpression(ae) = nodes.kind(gp) {
                if ae.left.span() == member.span {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Template walk
// ---------------------------------------------------------------------------

fn check_templates<'a>(
    ctx: &mut LintContext<'a>,
    template_targets: &HashMap<&'static str, HashSet<String>>,
) {
    let mut diagnostics: Vec<(Span, &'static BuiltinClass)> = Vec::new();
    walk_template_nodes(&ctx.ast.html, &mut |node| {
        if let TemplateNode::MustacheTag(tag) = node {
            scan_mustache(tag, template_targets, &mut diagnostics);
        }
        // Note: attribute-value expressions (`value={…}`, `style:foo={…}`)
        // are still strings in the template AST today — no fixtures exercise
        // tracked-binding mutation inside an attribute value, so we don't
        // re-parse them here. If needed later, we'd thread the same
        // `walk_expr` through `parse_template_expression`.
    });

    for (span, builtin) in diagnostics {
        ctx.diagnostic(message(builtin), span);
    }
}

fn scan_mustache<'a>(
    tag: &MustacheTag<'a>,
    template_targets: &HashMap<&'static str, HashSet<String>>,
    out: &mut Vec<(Span, &'static BuiltinClass)>,
) {
    let Some(expr) = tag.expression_ast else {
        return;
    };
    walk_expr(expr, template_targets, tag.span, out);
}

/// Recursively scan an expression for two patterns:
/// 1. `new Builtin(...).<mutator>(...)` — a chained mutating call where the
///    receiver is a fresh construction.
/// 2. `<name>.<mutator>(...)` or `<name>.<prop> = ...` where `<name>` is in
///    `template_targets[builtin]` — a tracked binding mutated from inside
///    the template.
fn walk_expr<'a>(
    expr: &'a Expression<'a>,
    targets: &HashMap<&'static str, HashSet<String>>,
    span: Span,
    out: &mut Vec<(Span, &'static BuiltinClass)>,
) {
    match expr {
        Expression::CallExpression(ce) => {
            if let Expression::StaticMemberExpression(mem) = &ce.callee {
                check_template_member(&mem.object, mem.property.name.as_str(), false, targets, span, out);
                walk_expr(&mem.object, targets, span, out);
            } else {
                walk_expr(&ce.callee, targets, span, out);
            }
            for arg in &ce.arguments {
                if let Some(e) = arg.as_expression() {
                    walk_expr(e, targets, span, out);
                }
            }
        }
        Expression::AssignmentExpression(ae) => {
            if let AssignmentTarget::StaticMemberExpression(mem) = &ae.left {
                check_template_member(&mem.object, mem.property.name.as_str(), true, targets, span, out);
            }
            walk_expr(&ae.right, targets, span, out);
        }
        Expression::NewExpression(ne) => {
            for arg in &ne.arguments {
                if let Some(e) = arg.as_expression() {
                    walk_expr(e, targets, span, out);
                }
            }
        }
        Expression::ParenthesizedExpression(p) => walk_expr(&p.expression, targets, span, out),
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                walk_expr(e, targets, span, out);
            }
        }
        Expression::ConditionalExpression(c) => {
            walk_expr(&c.test, targets, span, out);
            walk_expr(&c.consequent, targets, span, out);
            walk_expr(&c.alternate, targets, span, out);
        }
        Expression::LogicalExpression(l) => {
            walk_expr(&l.left, targets, span, out);
            walk_expr(&l.right, targets, span, out);
        }
        Expression::BinaryExpression(b) => {
            walk_expr(&b.left, targets, span, out);
            walk_expr(&b.right, targets, span, out);
        }
        Expression::UnaryExpression(u) => walk_expr(&u.argument, targets, span, out),
        Expression::StaticMemberExpression(mem) => walk_expr(&mem.object, targets, span, out),
        _ => {}
    }
}

/// Common backend for the two template patterns above. `is_assign = true`
/// means we're handling `<receiver>.<prop> = ...`, otherwise `<receiver>.<method>(...)`.
fn check_template_member<'a>(
    receiver: &'a Expression<'a>,
    prop: &str,
    is_assign: bool,
    targets: &HashMap<&'static str, HashSet<String>>,
    span: Span,
    out: &mut Vec<(Span, &'static BuiltinClass)>,
) {
    let bare = peel(receiver);
    let matches = |b: &BuiltinClass| {
        if is_assign {
            b.mutating_props.contains(&prop)
        } else {
            b.mutating_methods.contains(&prop)
        }
    };
    // Pattern 1: chained on a fresh construction.
    if let Expression::NewExpression(ne) = bare {
        if let Expression::Identifier(id) = &ne.callee {
            if let Some(builtin) = lookup_builtin(id.name.as_str()) {
                if matches(builtin) {
                    out.push((span, builtin));
                    return;
                }
            }
        }
    }
    // Pattern 2: tracked-by-name binding.
    if let Expression::Identifier(id) = bare {
        let name = id.name.as_str();
        for builtin in BUILTIN_CLASSES.iter().filter(|b| matches(b)) {
            if targets
                .get(builtin.name)
                .is_some_and(|s| s.contains(name))
            {
                out.push((span, builtin));
                return;
            }
        }
    }
}

fn peel<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(p) => peel(&p.expression),
        _ => expr,
    }
}

