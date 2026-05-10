//! `svelte/no-immutable-reactive-statements` — flag `$: …` reactive statements
//! that reference only immutable values (so they never re-run). Recommended.
//!
//! Mirrors vendor's scope-manager-driven algorithm at
//! `vendors/eslint-plugin-svelte/.../no-immutable-reactive-statements.ts`.
//! Detection is AST + scope-manager driven on the script side. Template-side
//! member-write detection (`bind:`, `{#each x as ctx}`, member writes inside
//! attribute expressions like `on:click={() => x.y = …}`) walks `ctx.ast.html`
//! but inspects the small attribute-value text strings for write operators,
//! since attribute expressions aren't parsed to typed AST yet (Tier 2 work).
//! Vendor gates the rule to Svelte 3/4 or Svelte 5 with runes=false/undetermined;
//! we early-return when `ctx.is_runes` is true.

use crate::ast::{Attribute, DirectiveKind, EachBlock, Fragment, TemplateNode};
use crate::linter::{walk_template_nodes, LintContext, Rule};
use oxc::ast::ast::{Expression, Statement, VariableDeclarationKind};
use oxc::ast::AstKind;
use oxc::semantic::{NodeId, Reference, Semantic, SymbolId};
use oxc::span::{GetSpan, Span};
use std::collections::{HashMap, HashSet};

const MESSAGE: &str =
    "This statement is not reactive because all variables referenced in the reactive statement are immutable.";

pub struct NoImmutableReactiveStatements;

impl Rule for NoImmutableReactiveStatements {
    fn name(&self) -> &'static str {
        "svelte/no-immutable-reactive-statements"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Vendor gates to Svelte 3/4 OR Svelte 5 with runes=false/undetermined.
        if ctx.is_runes {
            return;
        }
        let Some(semantic) = ctx.instance_semantic else {
            return;
        };
        let content_offset = ctx.instance_content_offset;
        let template_writes = collect_template_writes(&ctx.ast.html);
        let mut cache: HashMap<SymbolId, bool> = HashMap::new();
        let mut findings: Vec<Span> = Vec::new();
        for stmt in &semantic.nodes().program().body {
            let Statement::LabeledStatement(ls) = stmt else {
                continue;
            };
            if ls.label.name.as_str() != "$" {
                continue;
            }
            if !range_is_immutable(semantic, ls.span, &template_writes, &mut cache) {
                continue;
            }
            // Vendor reports on the RHS of `$: x = expr;`, otherwise on the body.
            let report_span = match &ls.body {
                Statement::ExpressionStatement(es) => match &es.expression {
                    Expression::AssignmentExpression(ae) if ae.operator.as_str() == "=" => {
                        ae.right.span()
                    }
                    _ => ls.body.span(),
                },
                _ => ls.body.span(),
            };
            findings.push(Span::new(
                content_offset + report_span.start,
                content_offset + report_span.end,
            ));
        }
        for span in findings {
            ctx.diagnostic(MESSAGE, span);
        }
    }
}

/// Returns `true` iff every (non-write-only) reference inside `range` resolves
/// to an immutable variable, AND there are no `$$`-prefixed or unresolved
/// identifiers inside `range` (vendor's `toplevelScope.through` filter).
fn range_is_immutable<'a>(
    semantic: &'a Semantic<'a>,
    range: Span,
    template_writes: &HashSet<String>,
    cache: &mut HashMap<SymbolId, bool>,
) -> bool {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();

    // Single pass over IdentifierReferences inside `range`. Combines vendor's
    // `iterateRangeReferences` loop and the `toplevelScope.through` filter:
    //   - skip write-only references (vendor's `isWriteOnly` short-circuit;
    //     also covers oxc's unresolved `$: foo = …` LHS, since
    //     svelte-eslint-parser auto-binds those but oxc doesn't);
    //   - bail on a `$`-prefixed name (Svelte store ref, or `$$`-prefixed
    //     builtin like `$$props`);
    //   - bail on any unresolved read (vendor's `!reference.resolved`),
    //     except names in our `is_known_global` allowlist (vendor's setup
    //     resolves browser/JS globals to implicit-global ESLint Variables);
    //   - bail on a resolved-mutable variable (vendor's `isMutableVariable`).
    for node in nodes.iter() {
        let AstKind::IdentifierReference(id) = node.kind() else {
            continue;
        };
        let span = id.span;
        if span.start < range.start || span.end > range.end {
            continue;
        }
        let Some(rid) = id.reference_id.get() else {
            return false;
        };
        let reference: &Reference = scoping.get_reference(rid);
        if reference.is_write() && !reference.is_read() {
            continue;
        }
        if id.name.starts_with('$') && id.name.as_str() != "$" {
            return false;
        }
        match reference.symbol_id() {
            None if is_known_global(&id.name) => {}
            None => return false,
            Some(sid) if is_mutable_variable(semantic, sid, template_writes, cache) => {
                return false;
            }
            Some(_) => {}
        }
    }
    true
}

/// Names of well-known browser / JS globals. ESLint's scope-manager
/// resolves these to implicit-global Variables (so vendor's
/// `reference.resolved == null` check is false for them); oxc doesn't
/// synthesize implicit globals, so we recognise them by name.
fn is_known_global(name: &str) -> bool {
    const GLOBALS: &[&str] = &[
        // Standard JS globals.
        "globalThis",
        "undefined",
        "Infinity",
        "NaN",
        "Object",
        "Array",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "Promise",
        "Date",
        "Math",
        "JSON",
        "Number",
        "String",
        "Boolean",
        "Symbol",
        "RegExp",
        "Error",
        "TypeError",
        "RangeError",
        "SyntaxError",
        "ReferenceError",
        "EvalError",
        "URIError",
        "Function",
        "ArrayBuffer",
        "DataView",
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float32Array",
        "Float64Array",
        "BigInt",
        "BigInt64Array",
        "BigUint64Array",
        "Reflect",
        "Proxy",
        "parseInt",
        "parseFloat",
        "isNaN",
        "isFinite",
        "encodeURI",
        "decodeURI",
        "encodeURIComponent",
        "decodeURIComponent",
        // Browser / runtime globals.
        "console",
        "window",
        "document",
        "self",
        "navigator",
        "history",
        "location",
        "screen",
        "localStorage",
        "sessionStorage",
        "indexedDB",
        "fetch",
        "alert",
        "confirm",
        "prompt",
        "setTimeout",
        "setInterval",
        "clearTimeout",
        "clearInterval",
        "queueMicrotask",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "URL",
        "URLSearchParams",
        "FormData",
        "Blob",
        "File",
        "FileReader",
        "Headers",
        "Request",
        "Response",
        "Event",
        "CustomEvent",
        "AbortController",
        "AbortSignal",
        "structuredClone",
        "atob",
        "btoa",
        "crypto",
        "performance",
        "TextEncoder",
        "TextDecoder",
        "process",
        "module",
        "require",
        "Buffer",
    ];
    GLOBALS.contains(&name)
}

/// Vendor `isMutableVariable`. Memoized per symbol.
fn is_mutable_variable<'a>(
    semantic: &'a Semantic<'a>,
    sid: SymbolId,
    template_writes: &HashSet<String>,
    cache: &mut HashMap<SymbolId, bool>,
) -> bool {
    if let Some(&cached) = cache.get(&sid) {
        return cached;
    }
    let result = compute_mutability(semantic, sid, template_writes);
    cache.insert(sid, result);
    result
}

/// Vendor's `isMutableVariable` enumerates `variable.defs`; oxc symbols have
/// one declaration site, but its surrounding AST exposes the same info.
fn compute_mutability<'a>(
    semantic: &'a Semantic<'a>,
    sid: SymbolId,
    template_writes: &HashSet<String>,
) -> bool {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let decl_id = scoping.symbol_declaration(sid);
    let decl_kind = nodes.kind(decl_id);

    // Imports → immutable.
    if matches!(
        decl_kind,
        AstKind::ImportSpecifier(_)
            | AstKind::ImportDefaultSpecifier(_)
            | AstKind::ImportNamespaceSpecifier(_)
    ) {
        return false;
    }

    // Chain `decl_id` itself: oxc may set `symbol_declaration` to either the
    // BindingIdentifier or the VariableDeclarator depending on shape.
    let chain = std::iter::once(decl_id).chain(nodes.ancestor_ids(decl_id));
    let mut declarator: Option<&oxc::ast::ast::VariableDeclarator> = None;
    let mut declaration: Option<&oxc::ast::ast::VariableDeclaration> = None;
    let mut exported = false;
    for aid in chain {
        match nodes.kind(aid) {
            AstKind::ImportDeclaration(_) => return false,
            // `function foo() {}` / `class Foo {}` — vendor: not AssignmentExpression,
            // not Variable, not ImportBinding → falls through → immutable.
            AstKind::Function(f) if f.is_declaration() => return false,
            AstKind::Class(_) | AstKind::TSEnumDeclaration(_) => return false,
            // `$: foo = expr` — vendor: def.node === 'AssignmentExpression' → mutable.
            AstKind::AssignmentExpression(_) => return true,
            AstKind::VariableDeclarator(d) => {
                if declarator.is_none() {
                    declarator = Some(d);
                }
            }
            AstKind::VariableDeclaration(vd) => {
                if declaration.is_none() {
                    declaration = Some(vd);
                }
            }
            AstKind::ExportNamedDeclaration(_) => exported = true,
            AstKind::Program(_) | AstKind::BlockStatement(_) => break,
            _ => {}
        }
    }
    if let Some(vd) = declaration {
        if vd.kind == VariableDeclarationKind::Const {
            if declarator
                .and_then(|d| d.init.as_ref())
                .is_some_and(|init| {
                    matches!(
                        init,
                        Expression::FunctionExpression(_)
                            | Expression::ArrowFunctionExpression(_)
                            | Expression::StringLiteral(_)
                            | Expression::NumericLiteral(_)
                            | Expression::BooleanLiteral(_)
                            | Expression::NullLiteral(_)
                            | Expression::BigIntLiteral(_)
                            | Expression::RegExpLiteral(_)
                    )
                })
            {
                return false;
            }
            return has_write_anywhere(semantic, sid, template_writes);
        }
        // let / var
        return exported || has_write_anywhere(semantic, sid, template_writes);
    }
    has_write_anywhere(semantic, sid, template_writes)
}

/// Walk the symbol's resolved references; return `true` iff any:
///   - is a write reference outside the def's binding span, OR
///   - parents into an `AssignmentExpression` LHS through a member chain
///     (or `UpdateExpression`, `delete x.y`), OR
///   - the symbol's name appears in `template_writes` (bind: directive on it,
///     or each-block-iterable with written context).
fn has_write_anywhere<'a>(
    semantic: &'a Semantic<'a>,
    sid: SymbolId,
    template_writes: &HashSet<String>,
) -> bool {
    let scoping = semantic.scoping();
    let name = scoping.symbol_name(sid);
    if template_writes.contains(name) {
        return true;
    }

    let nodes = semantic.nodes();
    let decl_node_id = scoping.symbol_declaration(sid);
    let decl_span = nodes.kind(decl_node_id).span();

    for reference in scoping.get_resolved_references(sid) {
        let ref_node_id = reference.node_id();
        let ref_span = nodes.kind(ref_node_id).span();

        // Skip the def site itself.
        if ref_span.start >= decl_span.start && ref_span.end <= decl_span.end {
            continue;
        }

        if reference.is_write() {
            return true;
        }

        // Member-write: walk the parent chain looking for an AssignmentExpression
        // / UpdateExpression / `delete x.y`.
        if has_write_member(nodes, ref_node_id) {
            return true;
        }
    }

    false
}

/// Vendor's `hasWriteMember`, AST-side. Walks the parent chain from the
/// identifier reference up through `MemberExpression` to find an
/// `AssignmentExpression` LHS, `UpdateExpression` argument, or `delete` UnaryExpression.
/// Vendor's `hasWriteMember`: walks the parent chain from a reference
/// through `MemberExpression`s into an `AssignmentExpression` LHS,
/// `UpdateExpression` argument, or `delete x.y`.
fn has_write_member<'a>(nodes: &oxc::semantic::AstNodes<'a>, ref_node_id: NodeId) -> bool {
    let mut cur = ref_node_id;
    let mut span = nodes.kind(cur).span();
    for _ in 0..64 {
        let parent_id = nodes.parent_id(cur);
        match nodes.kind(parent_id) {
            AstKind::AssignmentExpression(ae) => return ae.left.span() == span,
            AstKind::UpdateExpression(ue) => return ue.argument.span() == span,
            AstKind::UnaryExpression(un) => {
                return un.operator.as_str() == "delete" && un.argument.span() == span;
            }
            AstKind::StaticMemberExpression(_)
            | AstKind::ComputedMemberExpression(_)
            | AstKind::PrivateFieldExpression(_) => {
                if parent_id == cur {
                    return false;
                }
                cur = parent_id;
                span = nodes.kind(cur).span();
            }
            _ => return false,
        }
    }
    false
}

// ─── Template-side write detection ──────────────────────────────────────────

/// Walks the template AST and returns the set of *symbol names* that have at
/// least one template-side write — directly (`bind:foo={x}`) or via a member
/// (`x.y = …`, `x[i]++`, etc. inside any attribute / mustache text), or
/// indirectly as the iterable of an `{#each x as ctx}` whose body writes
/// to `ctx`.
///
/// Attribute expressions and each-block context patterns aren't parsed to
/// typed AST yet (Tier 2); we walk the template AST for the structural
/// detection (bind directive, each block, etc.) and inspect the small text
/// fields for the write operator. Replace with typed attribute ASTs once
/// available.
fn collect_template_writes(html: &Fragment) -> HashSet<String> {
    let mut writes: HashSet<String> = HashSet::new();
    let mut texts: Vec<String> = Vec::new();
    walk_template_nodes(html, &mut |node| match node {
        TemplateNode::Element(el) => {
            for attr in &el.attributes {
                match attr {
                    Attribute::Directive {
                        kind: DirectiveKind::Binding,
                        name,
                        value,
                        ..
                    } => {
                        let value_text = match value {
                            crate::ast::AttributeValue::True => Some(name.clone()),
                            _ => attr_value_text(value),
                        };
                        if let Some(ident) = leading_ident(value_text.as_deref().unwrap_or("")) {
                            writes.insert(ident);
                        }
                        if let Some(t) = attr_value_text(value) {
                            texts.push(t);
                        }
                    }
                    Attribute::Directive { value, .. }
                    | Attribute::NormalAttribute { value, .. } => {
                        if let Some(t) = attr_value_text(value) {
                            texts.push(t);
                        }
                    }
                    _ => {}
                }
            }
        }
        TemplateNode::EachBlock(block) => {
            // Each-block iterable is mutable iff any name in `context` has a
            // write reference inside the body. Approximation: if the body has
            // any bind: directive, OR any attribute/mustache text writes to
            // a name extracted from the context destructure, the iterable is
            // mutable.
            if each_body_writes_to_context(block) {
                if let Some(base) = leading_ident(&block.expression) {
                    writes.insert(base);
                }
            }
            texts.push(block.expression.clone());
        }
        TemplateNode::MustacheTag(tag) => texts.push(tag.expression.clone()),
        TemplateNode::RawMustacheTag(tag) => texts.push(tag.expression.clone()),
        TemplateNode::IfBlock(b) => texts.push(b.test.clone()),
        TemplateNode::AwaitBlock(b) => texts.push(b.expression.clone()),
        TemplateNode::KeyBlock(b) => texts.push(b.expression.clone()),
        _ => {}
    });
    for text in &texts {
        for name in find_member_writes(text) {
            writes.insert(name);
        }
    }
    writes
}

fn attr_value_text(v: &crate::ast::AttributeValue) -> Option<String> {
    use crate::ast::{AttributeValue, AttributeValuePart};
    match v {
        AttributeValue::Expression(t) => Some(t.clone()),
        AttributeValue::Concat(parts) => {
            let mut out = String::new();
            for p in parts {
                if let AttributeValuePart::Expression(t) = p {
                    out.push_str(t);
                    out.push('\n');
                }
            }
            (!out.is_empty()).then_some(out)
        }
        _ => None,
    }
}

/// True if any name in `block.context`'s destructure pattern is written to
/// inside the block's body (vendor's `hasWriteReference(parent.context)`).
/// Uses textual scanning since attribute expressions aren't typed yet.
fn each_body_writes_to_context(block: &EachBlock) -> bool {
    let names = extract_idents(&block.context);
    if names.is_empty() {
        return false;
    }
    let mut hit = false;
    walk_template_nodes(&block.body, &mut |node| {
        if hit {
            return;
        }
        let mut texts: Vec<&str> = Vec::new();
        match node {
            TemplateNode::Element(el) => {
                for attr in &el.attributes {
                    if let Attribute::Directive {
                        kind: DirectiveKind::Binding,
                        ..
                    } = attr
                    {
                        hit = true;
                        return;
                    }
                    if let Attribute::Directive { value, .. }
                    | Attribute::NormalAttribute { value, .. } = attr
                    {
                        if let crate::ast::AttributeValue::Expression(t) = value {
                            texts.push(t.as_str());
                        }
                    }
                }
            }
            TemplateNode::MustacheTag(tag) => texts.push(tag.expression.as_str()),
            TemplateNode::RawMustacheTag(tag) => texts.push(tag.expression.as_str()),
            _ => return,
        }
        if texts.iter().any(|t| names.iter().any(|n| writes_to(t, n))) {
            hit = true;
        }
    });
    hit
}

/// Find `name.X = …` / `name[…] = …` / `++name.x` / `delete name.x` patterns
/// in `text` and return the leading identifier names. Narrow textual fallback
/// for attribute/mustache expression text, which isn't parsed to typed AST yet.
fn find_member_writes(text: &str) -> Vec<String> {
    scan_writes(text, None)
}

/// Returns true iff `text` contains a write to the bare identifier `name`
/// (assignment, compound, ++/--). Word-bounded.
fn writes_to(text: &str, name: &str) -> bool {
    !scan_writes(text, Some(name)).is_empty()
}

/// Scan `text` for write patterns. If `target` is `Some(name)`, only match
/// bare identifiers equal to `name` (no member-chain required); else match
/// any identifier followed by a member chain (`.x` / `[…]`).
fn scan_writes(text: &str, target: Option<&str>) -> Vec<String> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < len {
        if !ident_start(bytes[i]) {
            i += 1;
            continue;
        }
        if i > 0 && (ident_cont(bytes[i - 1]) || bytes[i - 1] == b'.') {
            while i < len && ident_cont(bytes[i]) {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < len && ident_cont(bytes[i]) {
            i += 1;
        }
        let name = &text[start..i];
        if target.is_some_and(|t| t != name) {
            continue;
        }
        // Walk optional member chain: `.x`, `[…]`.
        let mut j = i;
        let mut had_member = false;
        loop {
            while j < len && matches!(bytes[j], b' ' | b'\t') {
                j += 1;
            }
            match bytes.get(j) {
                Some(b'.') => {
                    j += 1;
                    while j < len && matches!(bytes[j], b' ' | b'\t') {
                        j += 1;
                    }
                    let id0 = j;
                    while j < len && ident_cont(bytes[j]) {
                        j += 1;
                    }
                    if j == id0 {
                        break;
                    }
                    had_member = true;
                }
                Some(b'[') => match find_close(bytes, j) {
                    Some(c) => {
                        j = c + 1;
                        had_member = true;
                    }
                    None => break,
                },
                _ => break,
            }
        }
        // Bare-name mode: must NOT have a member chain (member chain → not
        // a write to the bare name itself).
        if target.is_some() && had_member {
            continue;
        }
        // Member-mode: must HAVE a member chain.
        if target.is_none() && !had_member {
            continue;
        }
        while j < len && matches!(bytes[j], b' ' | b'\t') {
            j += 1;
        }
        let prefix_update = start >= 2 && matches!(&bytes[start - 2..start], b"++" | b"--");
        let delete_prefix = start >= 7
            && bytes[start - 1] == b' '
            && bytes[start.saturating_sub(7)..start - 1].ends_with(b"delete");
        if is_write_op(bytes, j) || prefix_update || (had_member && delete_prefix) {
            out.push(name.to_string());
        }
    }
    out
}

fn find_close(bytes: &[u8], from: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = from;
    let mut in_str: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => in_str = Some(c),
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[inline]
fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}
#[inline]
fn ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// JS write operator at `i`? Skips `==`, `===`, `=>`, `!=`, `!==`, `<=`, `>=`.
fn is_write_op(bytes: &[u8], i: usize) -> bool {
    let Some(&c) = bytes.get(i) else { return false };
    let n = bytes.get(i + 1).copied();
    let n2 = bytes.get(i + 2).copied();
    match (c, n, n2) {
        (b'=', n, _) if !matches!(n, Some(b'=') | Some(b'>')) => true,
        (b'+', Some(b'+'), _) | (b'-', Some(b'-'), _) => true,
        (b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'&' | b'|', Some(b'='), _) => true,
        (b'*', Some(b'*'), Some(b'=')) => true,
        (b'&', Some(b'&'), Some(b'=')) => true,
        (b'|', Some(b'|'), Some(b'=')) => true,
        (b'?', Some(b'?'), Some(b'=')) => true,
        (b'<', Some(b'<'), Some(b'=')) => true,
        (b'>', Some(b'>'), Some(b'=')) => true,
        _ => false,
    }
}

/// Best-effort: leading identifier in `text`. `values[0]` → `values`, `obj.b` → `obj`.
fn leading_ident(text: &str) -> Option<String> {
    let s = text.trim();
    let bytes = s.as_bytes();
    if bytes.is_empty() || !ident_start(bytes[0]) {
        return None;
    }
    let mut end = 1;
    while end < bytes.len() && ident_cont(bytes[end]) {
        end += 1;
    }
    Some(s[..end].to_string())
}

/// Pull identifier-shaped tokens out of an each-block context destructure
/// (`value`, `{ a }`, `[x, y]`, `{ a: b }` — `b` is the binding, `a` is the key).
fn extract_idents(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    while i < len {
        let c = bytes[i];
        if let Some(q) = in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => {
                in_str = Some(c);
                i += 1;
            }
            b'=' => {
                // Default-value RHS — skip until next destructure separator.
                i += 1;
                while i < len && !matches!(bytes[i], b',' | b'}' | b']') {
                    i += 1;
                }
            }
            _ if ident_start(c) => {
                let start = i;
                i += 1;
                while i < len && ident_cont(bytes[i]) {
                    i += 1;
                }
                let name = &text[start..i];
                let mut j = i;
                while j < len && matches!(bytes[j], b' ' | b'\t') {
                    j += 1;
                }
                if matches!(bytes.get(j), Some(b':')) {
                    // `{ a: b }` — `a` is the key, skip it.
                    i = j + 1;
                    continue;
                }
                if !matches!(
                    name,
                    "true" | "false" | "null" | "undefined" | "this" | "in" | "of"
                ) {
                    out.push(name.to_string());
                }
            }
            _ => i += 1,
        }
    }
    out
}
