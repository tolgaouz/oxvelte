//! `svelte/infinite-reactive-loop` — detect reactive statements that may cause infinite loops.
//! ⭐ Recommended
//!
//! Walks each `$: ...` reactive statement's AST. Flags assignments to top-level variables
//! that are also referenced (non-call) in the same reactive statement, when the assignment
//! is separated from the reactive statement's start by a "microtask boundary": `await`,
//! Promise `.then`/`.catch` callbacks, or scheduled callbacks of `setTimeout`,
//! `setInterval`, `queueMicrotask`, and Svelte's `tick` (including local aliases/imports).

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::*;
use oxc::ast::AstKind;
use oxc::ast_visit::Visit;
use oxc::semantic::{NodeId, Semantic, SymbolId};
use oxc::span::{GetSpan, Ident, Span};
use rustc_hash::FxHashSet;

pub struct InfiniteReactiveLoop;

impl Rule for InfiniteReactiveLoop {
    fn name(&self) -> &'static str {
        "svelte/infinite-reactive-loop"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let program = semantic.nodes().program();

        // Fast bail: no `$:` labeled statements.
        let has_reactive = program
            .body
            .iter()
            .any(|s| matches!(s, Statement::LabeledStatement(ls) if ls.label.name == "$"));
        if !has_reactive {
            return;
        }

        // Pre-compute spans that mark "different microtask" regions: calls to `tick`,
        // `setTimeout`, `setInterval`, `queueMicrotask` and their top-level aliases.
        let task_call_spans = collect_task_scheduler_call_spans(semantic);
        // Module-script top-level names (used for cross-script name matching).
        let module_top_names = collect_module_top_level_names(ctx.module_semantic);
        // Names implicitly declared via reactive assignments `$: foo = ...`. These
        // track only within their declaring reactive body (no recursion).
        let reactive_declared_names = collect_reactive_declared_names(semantic);

        let content_offset = ctx.instance_content_offset;
        for stmt in &program.body {
            let Statement::LabeledStatement(ls) = stmt else { continue };
            if ls.label.name != "$" {
                continue;
            }
            check_reactive_statement(
                ctx, semantic, content_offset, ls,
                &task_call_spans, &module_top_names, &reactive_declared_names,
            );
        }
    }
}

// ─── Task-scheduler discovery ────────────────────────────────────────────────

/// Collect spans of all call expressions whose callee resolves to a task-scheduling
/// function: `tick` (from `svelte`), `setTimeout`, `setInterval`, `queueMicrotask`,
/// or a top-level `const` alias of any of these.
fn collect_task_scheduler_call_spans<'a>(semantic: &'a Semantic<'a>) -> Vec<Span> {
    let nodes = semantic.nodes();
    let program = nodes.program();

    // Names considered task schedulers. Seeded with the three globals; augmented
    // with imports of `tick` from `svelte` and top-level aliases.
    let mut names: FxHashSet<&str> = ["setTimeout", "setInterval", "queueMicrotask"]
        .iter()
        .copied()
        .collect();

    // 1) Imports of `tick` from `svelte`.
    for stmt in &program.body {
        if let Statement::ImportDeclaration(imp) = stmt {
            if imp.source.value == "svelte" {
                if let Some(specifiers) = &imp.specifiers {
                    for spec in specifiers {
                        if let ImportDeclarationSpecifier::ImportSpecifier(s) = spec {
                            let imported = match &s.imported {
                                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                                ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                                ModuleExportName::StringLiteral(l) => l.value.as_str(),
                            };
                            if imported == "tick" {
                                names.insert(s.local.name.as_str());
                            }
                        }
                    }
                }
            }
        }
    }

    // 2) Top-level `const foo = tick;` / `const foo = setTimeout;` aliases.
    //    Multi-pass until fixpoint so chains (`const a = setTimeout; const b = a`) resolve.
    loop {
        let before = names.len();
        for stmt in &program.body {
            let Statement::VariableDeclaration(vd) = stmt else { continue };
            for d in &vd.declarations {
                let BindingPattern::BindingIdentifier(local) = &d.id else { continue };
                let Some(init) = &d.init else { continue };
                if let Expression::Identifier(src) = init {
                    if names.contains(src.name.as_str()) {
                        names.insert(local.name.as_str());
                    }
                }
            }
        }
        if names.len() == before {
            break;
        }
    }

    // 3) Walk all CallExpressions; record span if callee is one of our names.
    let mut spans = Vec::new();
    for node in nodes.iter() {
        let AstKind::CallExpression(ce) = node.kind() else { continue };
        if let Expression::Identifier(callee) = &ce.callee {
            if names.contains(callee.name.as_str()) {
                spans.push(ce.span);
            }
        }
    }
    spans
}

// ─── Tracked reference discovery ─────────────────────────────────────────────

/// Names of top-level variables declared in the *module* script only (the
/// `<script module>` / `<script context="module">` block). These are visible to
/// the instance script as unresolved references, so we track them by name.
fn collect_module_top_level_names<'a>(module: Option<&'a Semantic<'a>>) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let Some(sem) = module else { return names };
    let scoping = sem.scoping();
    for sid in scoping.iter_bindings_in(scoping.root_scope_id()) {
        names.insert(scoping.symbol_name(sid).to_string());
    }
    names
}

/// Names of variables implicitly declared via reactive assignments at the top
/// level: `$: foo = expr;`. oxc doesn't create a binding for `foo` in that case
/// (it's a write to an undeclared identifier syntactically), but Svelte treats
/// `foo` as a reactive variable, and vendor tracks it as top-level.
fn collect_reactive_declared_names<'a>(semantic: &'a Semantic<'a>) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let program = semantic.nodes().program();
    for stmt in &program.body {
        let Statement::LabeledStatement(ls) = stmt else { continue };
        if ls.label.name != "$" {
            continue;
        }
        // Direct form: `$: foo = expr` → body is ExpressionStatement(AssignmentExpression)
        if let Statement::ExpressionStatement(es) = &ls.body {
            if let Expression::AssignmentExpression(ae) = &es.expression {
                if let AssignmentTarget::AssignmentTargetIdentifier(id) = &ae.left {
                    names.insert(id.name.to_string());
                }
            }
        }
    }
    names
}

/// For a given reactive-statement range, collect the sets used for tracking:
///  - `tracked_symbols`: instance-script top-level symbols whose (resolved)
///    references appear non-call inside the range.
///  - `tracked_names_module`: names referencing cross-script (module-script)
///    top-level vars or `$store` auto-subscriptions. Matched against unresolved
///    refs both inside the reactive body AND inside recursed helper functions.
///  - `tracked_names_reactive_all`: reactive-declared names (`$: foo = ...`)
///    referenced *anywhere* (read or write) in the reactive body. Matched
///    against unresolved refs inside the reactive body (`is_outer` walk).
///  - `tracked_names_reactive_read`: subset with at least one NON-WRITE
///    reference. Matched against unresolved refs inside recursed helpers.
///    Matches vendor behavior: a write-only reactive-declared name
///    (like `$: foo = expr;` with no re-read) does NOT propagate through
///    helper-function recursion.
fn collect_tracked<'a>(
    semantic: &'a Semantic<'a>,
    range: Span,
    module_top_names: &FxHashSet<String>,
    reactive_declared_names: &FxHashSet<String>,
) -> (FxHashSet<SymbolId>, FxHashSet<String>, FxHashSet<String>, FxHashSet<String>) {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let root_scope = scoping.root_scope_id();

    let mut tracked_symbols = FxHashSet::default();
    let mut tracked_names_module: FxHashSet<String> = FxHashSet::default();
    let mut tracked_names_reactive_all: FxHashSet<String> = FxHashSet::default();
    let mut tracked_names_reactive_read: FxHashSet<String> = FxHashSet::default();

    // (a) Scope-aware tracking of instance-script top-level symbols.
    for sid in scoping.iter_bindings_in(root_scope) {
        for reference in scoping.get_resolved_references(sid) {
            let ref_node_id = reference.node_id();
            let ref_span = nodes.kind(ref_node_id).span();
            if ref_span.start < range.start || ref_span.end > range.end {
                continue;
            }
            let parent_kind = nodes.parent_kind(ref_node_id);
            let is_call_callee = matches!(parent_kind,
                AstKind::CallExpression(ce) if ce.callee.span() == ref_span);
            if is_call_callee {
                continue;
            }
            tracked_symbols.insert(sid);
            break;
        }
    }

    let is_write_ref = |id_span: Span, parent: AstKind<'a>| -> bool {
        match parent {
            AstKind::AssignmentExpression(ae) => ae.left.span() == id_span,
            AstKind::UpdateExpression(ue) => ue.argument.span() == id_span,
            _ => false,
        }
    };

    for node in nodes.iter() {
        let AstKind::IdentifierReference(id) = node.kind() else { continue };
        let span = id.span;
        if span.start < range.start || span.end > range.end {
            continue;
        }
        let parent_kind = nodes.parent_kind(id.node_id.get());
        let is_call_callee = matches!(parent_kind,
            AstKind::CallExpression(ce) if ce.callee.span() == span);
        if is_call_callee {
            continue;
        }
        if scoping.get_reference(id.reference_id()).symbol_id().is_some() {
            continue;
        }
        let name = id.name.as_str();
        if module_top_names.contains(name) {
            tracked_names_module.insert(name.to_string());
        }
        if reactive_declared_names.contains(name) {
            tracked_names_reactive_all.insert(name.to_string());
            if !is_write_ref(span, parent_kind) {
                tracked_names_reactive_read.insert(name.to_string());
            }
        }
        if name.starts_with('$') && name.len() > 1 {
            let base = &name[1..];
            if scoping.find_binding(root_scope, Ident::new_const(base)).is_some()
                || module_top_names.contains(base)
            {
                tracked_names_module.insert(name.to_string());
            }
        }
    }

    (tracked_symbols, tracked_names_module, tracked_names_reactive_all, tracked_names_reactive_read)
}

// ─── Reactive-statement walk ────────────────────────────────────────────────

fn check_reactive_statement<'a>(
    ctx: &mut LintContext<'a>,
    semantic: &'a Semantic<'a>,
    content_offset: u32,
    ls: &'a LabeledStatement<'a>,
    task_call_spans: &[Span],
    module_top_names: &FxHashSet<String>,
    reactive_declared_names: &FxHashSet<String>,
) {
    let (tracked_symbols, tracked_names_module, tracked_names_reactive_all, tracked_names_reactive_read) =
        collect_tracked(semantic, ls.span, module_top_names, reactive_declared_names);
    if tracked_symbols.is_empty()
        && tracked_names_module.is_empty()
        && tracked_names_reactive_all.is_empty()
    {
        return;
    }

    // Pre-compute Promise `.then`/`.catch` callback spans. A "callback" is the
    // argument function (arrow or function expression) passed to `.then`/`.catch`.
    let promise_cb_spans = collect_promise_callback_spans(semantic);
    // Pre-compute LHS spans of `x = await y` style assignments. The LHS is
    // considered microtask-different (the value being written was computed after
    // an await boundary).
    let await_lhs_spans = collect_await_lhs_spans(semantic);

    let mut v = MicrotaskVisitor {
        ctx,
        semantic,
        content_offset,
        tracked_symbols: &tracked_symbols,
        tracked_names_module: &tracked_names_module,
        tracked_names_reactive_all: &tracked_names_reactive_all,
        tracked_names_reactive_read: &tracked_names_reactive_read,
        task_call_spans,
        promise_cb_spans: &promise_cb_spans,
        await_lhs_spans: &await_lhs_spans,
        node_stack: Vec::with_capacity(32),
        frames: vec![FunctionFrame::default()],
        call_chain: Vec::new(),
        visited_function_bodies: FxHashSet::default(),
        is_outer: true,
        is_same_microtask: true,
    };
    v.visit_statement(&ls.body);
}

/// Spans of function arguments to `.then` / `.catch` calls, i.e. Promise callbacks.
fn collect_promise_callback_spans<'a>(semantic: &'a Semantic<'a>) -> Vec<Span> {
    let mut out = Vec::new();
    for node in semantic.nodes().iter() {
        let AstKind::CallExpression(ce) = node.kind() else { continue };
        let Expression::StaticMemberExpression(mem) = &ce.callee else { continue };
        let prop = mem.property.name.as_str();
        if prop != "then" && prop != "catch" {
            continue;
        }
        for arg in &ce.arguments {
            let Some(expr) = arg.as_expression() else { continue };
            match expr {
                Expression::ArrowFunctionExpression(a) => out.push(a.span),
                Expression::FunctionExpression(f) => out.push(f.span),
                _ => {}
            }
        }
    }
    out
}

/// Spans of LHS expressions of `x = await y` / `x.p += await y` / etc. — the value
/// being assigned was produced after an await, so the LHS-at-write is microtask-different.
fn collect_await_lhs_spans<'a>(semantic: &'a Semantic<'a>) -> Vec<Span> {
    let mut out = Vec::new();
    for node in semantic.nodes().iter() {
        let AstKind::AssignmentExpression(ae) = node.kind() else { continue };
        if matches!(&ae.right, Expression::AwaitExpression(_)) {
            out.push(ae.left.span());
        }
    }
    out
}

// ─── Visitor ─────────────────────────────────────────────────────────────────

#[derive(Default)]
struct FunctionFrame {
    /// Has an AwaitExpression been passed (in document order) inside this function frame?
    await_seen: bool,
}

struct MicrotaskVisitor<'a, 'ctx> {
    ctx: &'ctx mut LintContext<'a>,
    semantic: &'a Semantic<'a>,
    content_offset: u32,
    tracked_symbols: &'ctx FxHashSet<SymbolId>,
    /// Names matched both in syntactic walks AND recursion (cross-script + stores).
    tracked_names_module: &'ctx FxHashSet<String>,
    /// Reactive-declared names (`$: foo = ...`) referenced anywhere in the
    /// reactive body. Matched against refs inside the reactive body (is_outer).
    tracked_names_reactive_all: &'ctx FxHashSet<String>,
    /// Subset of `tracked_names_reactive_all` with at least one non-write ref.
    /// Matched against refs inside recursed helper functions.
    tracked_names_reactive_read: &'ctx FxHashSet<String>,
    task_call_spans: &'ctx [Span],
    promise_cb_spans: &'ctx [Span],
    await_lhs_spans: &'ctx [Span],
    node_stack: Vec<AstKind<'a>>,
    frames: Vec<FunctionFrame>,
    /// Ordered list of outer call-callee identifier spans (innermost last). When we
    /// report an assignment inside a recursively-traversed function, each caller in
    /// the chain also gets an "unexpectedCall" diagnostic.
    call_chain: Vec<CallChainEntry<'a>>,
    visited_function_bodies: FxHashSet<NodeId>,
    /// True while walking the direct body of the reactive statement (the initial
    /// traversal). False inside recursed-into function bodies.
    is_outer: bool,
    /// Single boolean tracking "is current node in a microtask-different region".
    /// Flipped on enter/leave of marker nodes (promise callbacks, task-call
    /// descendants, await-LHS). Mirrors vendor's buggy-but-observed behavior
    /// where leaving a nested marker restores to true even while still inside an
    /// outer marker region.
    is_same_microtask: bool,
}

struct CallChainEntry<'a> {
    id: &'a IdentifierReference<'a>,
}

impl<'a, 'ctx> MicrotaskVisitor<'a, 'ctx> {
    /// Is the current node positioned such that execution reaches it only after
    /// a microtask boundary relative to the reactive-statement start?
    fn is_microtask_different(&self, _span: Span) -> bool {
        !self.is_same_microtask
            || self.frames.last().map_or(false, |f| f.await_seen)
    }

    /// Does `kind` qualify as a "different-microtask boundary marker"? Matches
    /// vendor's three conditions:
    ///   1. The node is a promise `.then`/`.catch` callback function.
    ///   2. The node is strictly inside a task-scheduler call (setTimeout, tick, ...).
    ///   3. The node's span equals a LHS of `x = await y`.
    fn is_marker_node(&self, kind: AstKind<'a>) -> bool {
        let span = kind.span();
        if self.promise_cb_spans.iter().any(|s| *s == span) {
            return true;
        }
        if self.task_call_spans.iter().any(|s| span.start > s.start && span.end <= s.end) {
            return true;
        }
        if self.await_lhs_spans.iter().any(|s| *s == span) {
            return true;
        }
        false
    }

    fn parent(&self) -> Option<AstKind<'a>> {
        let n = self.node_stack.len();
        if n < 2 {
            return None;
        }
        Some(self.node_stack[n - 2])
    }

    /// True if any ancestor is an async `FunctionDeclaration` or an async function
    /// expression assigned to a `VariableDeclarator` (`const f = async () => ...`).
    /// Awaits inside these don't propagate the microtask-different state outward.
    fn is_inside_named_async_function(&self) -> bool {
        for ancestor in self.node_stack.iter().rev() {
            match ancestor {
                AstKind::Function(f) if f.is_declaration() && f.r#async => return true,
                AstKind::VariableDeclarator(vd) => {
                    if let Some(init) = &vd.init {
                        match init {
                            Expression::ArrowFunctionExpression(a) if a.r#async => return true,
                            Expression::FunctionExpression(f) if f.r#async => return true,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn report_assignment(&mut self, id_span: Span, var_name: &str) {
        let abs_start = self.content_offset + id_span.start;
        let abs_end = self.content_offset + id_span.end;
        self.ctx.diagnostic(
            "Possibly it may occur an infinite reactive loop.",
            Span::new(abs_start, abs_end),
        );
        // Also report each caller in the chain.
        for entry in &self.call_chain {
            let s = self.content_offset + entry.id.span.start;
            let e = self.content_offset + entry.id.span.end;
            self.ctx.diagnostic(
                format!(
                    "Possibly it may occur an infinite reactive loop because this function may update `{}`.",
                    var_name
                ),
                Span::new(s, e),
            );
        }
    }

    /// True if this identifier is on the left-hand-side of any assignment
    /// (direct identifier LHS, or `id.prop = ...` / `id[x] = ...`).
    fn is_assignment_target(&self, id_span: Span) -> bool {
        let Some(parent) = self.parent() else { return false };
        match parent {
            // Direct: `id = ...`, `id += ...`
            AstKind::AssignmentExpression(ae) => ae.left.span() == id_span,
            // `id.prop = ...` or `id[x] = ...`
            AstKind::StaticMemberExpression(mem) => {
                if mem.object.span() != id_span {
                    return false;
                }
                self.grandparent_is_assignment_target(mem.span)
            }
            AstKind::ComputedMemberExpression(mem) => {
                if mem.object.span() != id_span {
                    return false;
                }
                self.grandparent_is_assignment_target(mem.span)
            }
            AstKind::UpdateExpression(_ue) => true, // id++, --id, etc.
            _ => false,
        }
    }

    fn grandparent_is_assignment_target(&self, member_span: Span) -> bool {
        let n = self.node_stack.len();
        if n < 3 {
            return false;
        }
        let gp = self.node_stack[n - 3];
        match gp {
            AstKind::AssignmentExpression(ae) => ae.left.span() == member_span,
            AstKind::UpdateExpression(_) => true,
            _ => false,
        }
    }

    /// Given a reference identifier at a call callee position, try to resolve it
    /// to a top-level function declaration/expression body and return its Statement.
    fn resolve_call_body(&self, id: &'a IdentifierReference<'a>) -> Option<CallableBody<'a>> {
        let scoping = self.semantic.scoping();
        let nodes = self.semantic.nodes();
        let symbol_id = scoping.get_reference(id.reference_id()).symbol_id()?;
        let decl_node_id = scoping.symbol_declaration(symbol_id);
        // Walk ancestors to find the nearest FunctionDeclaration or VariableDeclarator.
        for ancestor_id in std::iter::once(decl_node_id).chain(nodes.ancestor_ids(decl_node_id)) {
            match nodes.kind(ancestor_id) {
                AstKind::Function(func) if func.is_declaration() => {
                    let body = func.body.as_ref()?;
                    return Some(CallableBody { node_id: ancestor_id, body: CallableBodyKind::Block(body) });
                }
                AstKind::VariableDeclarator(vd) => {
                    let init = vd.init.as_ref()?;
                    return match init {
                        Expression::ArrowFunctionExpression(arr) => Some(CallableBody {
                            node_id: ancestor_id,
                            body: CallableBodyKind::Function(&arr.body),
                        }),
                        Expression::FunctionExpression(f) => f.body.as_ref().map(|b| CallableBody {
                            node_id: ancestor_id,
                            body: CallableBodyKind::Block(b),
                        }),
                        _ => None,
                    };
                }
                _ => {}
            }
        }
        None
    }

    fn recurse_into_function(&mut self, callee_id: &'a IdentifierReference<'a>, body: CallableBody<'a>) {
        if !self.visited_function_bodies.insert(body.node_id) {
            return;
        }
        // Carry over microtask-different state from the call site. If the call
        // itself is after an `await`, inside `setTimeout(...)`, etc., the called
        // function's body inherits that — anything it assigns is microtask-different
        // relative to the reactive statement.
        let carry_await = self.is_microtask_different(callee_id.span);
        self.call_chain.push(CallChainEntry { id: callee_id });
        let saved_outer = self.is_outer;
        let saved_is_same = self.is_same_microtask;
        self.is_outer = false;
        // Fresh marker state inside the recursed body.
        self.is_same_microtask = true;
        self.frames.push(FunctionFrame { await_seen: carry_await });
        let saved_stack_len = self.node_stack.len();

        match body.body {
            CallableBodyKind::Block(b) => self.visit_function_body(b),
            CallableBodyKind::Function(fb) => self.visit_function_body(fb),
        }

        // Restore — truncate stack in case any visit_* method left dangling pushes (shouldn't).
        self.node_stack.truncate(saved_stack_len);
        self.frames.pop();
        self.is_outer = saved_outer;
        self.is_same_microtask = saved_is_same;
        self.call_chain.pop();
        // Note: intentionally do NOT remove from visited_function_bodies. Within
        // one reactive statement's traversal, each function body is analyzed at
        // most once — matches vendor behavior and prevents duplicate reports when
        // the same function is called multiple times.
    }
}

struct CallableBody<'a> {
    node_id: NodeId,
    body: CallableBodyKind<'a>,
}

enum CallableBodyKind<'a> {
    Block(&'a FunctionBody<'a>),
    Function(&'a FunctionBody<'a>),
}

impl<'a, 'ctx> Visit<'a> for MicrotaskVisitor<'a, 'ctx> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        self.node_stack.push(kind);

        // If entering a microtask-boundary marker node, flip state.
        if self.is_marker_node(kind) {
            self.is_same_microtask = false;
        }

        // Note: we deliberately do NOT push a new function frame on entry into
        // inline function expressions. The vendor walks the reactive-statement
        // body with a single `isSameMicroTask` flag that persists across inline
        // function boundaries (so an `await` inside an inline arrow still marks
        // subsequent sibling statements as microtask-different). We only push a
        // fresh frame when we *recurse* into an externally-called function body
        // via `recurse_into_function`.

        // Check identifier references for:
        //   a) tracked assignment target → report
        //   b) call callee → recurse into callee's body
        if let AstKind::IdentifierReference(id) = kind {
            let id_span = id.span;
            let name = id.name.as_str();
            let scoping = self.semantic.scoping();

            // (a) Tracked assignment target under a microtask-different context?
            //     Scope-aware: prefer symbol-id resolution; fall back to name match
            //     only for UNRESOLVED refs (cross-script / stores / reactive-declared).
            //     Reactive-declared names match in recursion ONLY if they had a
            //     non-write reference in the reactive body.
            if self.is_microtask_different(id_span) && self.is_assignment_target(id_span) {
                let symbol_id = scoping.get_reference(id.reference_id()).symbol_id();
                let matched = match symbol_id {
                    Some(s) => self.tracked_symbols.contains(&s),
                    None => {
                        self.tracked_names_module.contains(name)
                            || if self.is_outer {
                                self.tracked_names_reactive_all.contains(name)
                            } else {
                                self.tracked_names_reactive_read.contains(name)
                            }
                    }
                };
                if matched {
                    self.report_assignment(id_span, name);
                }
            }

            // (b) Call callee → recurse. Only for direct `foo(...)` where this id is the callee.
            let parent = self.parent();
            let is_call_callee = matches!(parent,
                Some(AstKind::CallExpression(ce)) if ce.callee.span() == id_span);
            if is_call_callee {
                if let Some(body) = self.resolve_call_body(id) {
                    self.recurse_into_function(id, body);
                }
            }
        }
    }

    fn leave_node(&mut self, kind: AstKind<'a>) {
        // Update await-seen on the current frame — but only if the await is not
        // inside a nested *named* async function (one assigned to a variable or a
        // FunctionDeclaration). Awaits in those don't leak out because the
        // function may never actually be called; matches vendor's
        // `isInsideOfFunction` check.
        if matches!(kind, AstKind::AwaitExpression(_)) && !self.is_inside_named_async_function() {
            if let Some(top) = self.frames.last_mut() {
                top.await_seen = true;
            }
        }
        // Leaving a marker node restores microtask state — matches vendor's
        // buggy restore: `isSameMicroTask = true` even if still inside an
        // outer marker region.
        if self.is_marker_node(kind) {
            self.is_same_microtask = true;
        }
        self.node_stack.pop();
    }
}

