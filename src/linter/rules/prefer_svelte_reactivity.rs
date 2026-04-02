//! `svelte/prefer-svelte-reactivity` — prefer Svelte reactive classes over mutable
//! built-in JS classes (Date, Map, Set, URL, URLSearchParams).
//! ⭐ Recommended
//!
//! Uses `oxc_semantic` for scope-based analysis. For each `new BuiltinClass()`,
//! resolves the assigned variable symbol, then checks if the resulting value is
//! mutated before being overwritten by another assignment.

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct PreferSvelteReactivity;

struct BuiltinClass {
    name: &'static str,
    svelte_name: &'static str,
    mutating_methods: &'static [&'static str],
    mutating_props: &'static [&'static str],
}

const BUILTIN_CLASSES: &[BuiltinClass] = &[
    BuiltinClass {
        name: "Date",
        svelte_name: "SvelteDate",
        mutating_methods: &[
            "setDate", "setFullYear", "setHours", "setMilliseconds", "setMinutes",
            "setMonth", "setSeconds", "setTime", "setUTCDate", "setUTCFullYear",
            "setUTCHours", "setUTCMilliseconds", "setUTCMinutes", "setUTCMonth",
            "setUTCSeconds", "setYear",
        ],
        mutating_props: &[],
    },
    BuiltinClass {
        name: "Map",
        svelte_name: "SvelteMap",
        mutating_methods: &["set", "delete", "clear"],
        mutating_props: &[],
    },
    BuiltinClass {
        name: "Set",
        svelte_name: "SvelteSet",
        mutating_methods: &["add", "delete", "clear"],
        mutating_props: &[],
    },
    BuiltinClass {
        name: "URL",
        svelte_name: "SvelteURL",
        mutating_methods: &[],
        mutating_props: &[
            "hash", "host", "hostname", "href", "password",
            "pathname", "port", "protocol", "search", "username",
        ],
    },
    BuiltinClass {
        name: "URLSearchParams",
        svelte_name: "SvelteURLSearchParams",
        mutating_methods: &["append", "delete", "set", "sort"],
        mutating_props: &[],
    },
];

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
        for script in [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten() {
            let content = &script.content;
            if content.trim().is_empty() { continue; }

            // For .svelte.js/.svelte.ts modules (synthetic AST), content_offset is 0
            // since there's no <script> tag wrapper.
            let content_offset = if ctx.is_svelte_module {
                0
            } else {
                let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
                let gt = tag_text.find('>').unwrap_or(0);
                script.span.start as usize + gt + 1
            };

            let is_ts = script.lang.as_deref() == Some("ts")
                || script.lang.as_deref() == Some("typescript");

            self.check_script(ctx, content, content_offset, is_ts, ctx.is_svelte_module);
        }

        // Scan template (non-script) regions for chained mutations on new built-in
        // instances, e.g. `{@const d = new Date(x).setHours(0)}` or
        // `{new Date().setMonth(m)}` in template expressions / event handlers.
        if !ctx.is_svelte_module {
            detect_chained_new_in_template(ctx);
        }
    }
}

impl PreferSvelteReactivity {
    fn check_script(
        &self,
        ctx: &mut LintContext,
        content: &str,
        content_offset: usize,
        is_ts: bool,
        is_svelte_module: bool,
    ) {
        use oxc::allocator::Allocator;
        use oxc::ast::ast::{AssignmentTarget, Expression};
        use oxc::ast::AstKind;
        use oxc::parser::Parser;
        use oxc::semantic::{SemanticBuilder, SymbolId};
        use oxc::span::{GetSpan, SourceType};

        let alloc = Allocator::default();
        let source_type = if is_ts { SourceType::ts() } else { SourceType::mjs() };
        let parse_result = Parser::new(&alloc, content, source_type).parse();
        if !parse_result.errors.is_empty() { return; }

        let semantic_ret = SemanticBuilder::new().build(&parse_result.program);
        let semantic = semantic_ret.semantic;
        let scoping = semantic.scoping();
        let nodes = semantic.nodes();

        // Collect shadowed class names (imported from packages)
        let shadowed = collect_shadowed_classes(content);

        // The vendor's ReferenceTracker has a side-effect: due to
        // variableStack and lazy generator evaluation, only the FIRST
        // `new Set()` (in source order) per mutable variable is flagged.
        // We replicate this by tracking which symbols have already been
        // reported.
        let mut flagged_symbols = std::collections::HashSet::<SymbolId>::new();

        // For each NewExpression of a built-in class, check if the value is mutated.
        for ast_node in nodes.iter() {
            let AstKind::NewExpression(new_expr) = ast_node.kind() else { continue };
            let Expression::Identifier(callee) = &new_expr.callee else { continue };
            let class_name = callee.name.as_str();

            let Some(builtin) = BUILTIN_CLASSES.iter().find(|b| b.name == class_name) else { continue };
            if shadowed.contains(&class_name.to_string()) { continue; }

            // Check callee is unresolved (global built-in, not locally imported)
            if scoping.has_binding(callee.reference_id()) { continue; }

            // URL/URLSearchParams: not in ECMAScript globals. Skip when not at
            // root scope (the vendor only finds these with browser globals, and
            // even then only at module scope level in practice).
            if class_name == "URL" || class_name == "URLSearchParams" {
                if ast_node.scope_id() != scoping.root_scope_id() {
                    continue;
                }
            }

            // Skip if new Class() is inside a $state() call — $state already
            // provides reactivity, so SvelteMap/SvelteSet isn't needed.
            let new_node_id = callee.node_id.get();
            let new_node_id = nodes.parent_id(new_node_id); // up to NewExpression
            if is_inside_state_call(nodes, new_node_id) { continue; }

            // In .svelte.[js|ts] modules, exported `new Set()`/`new Map()` etc.
            // are always flagged regardless of mutation (the vendor checks
            // ExportNamedDeclaration/ExportDefaultDeclaration).
            if is_svelte_module && is_inside_export(nodes, new_node_id) {
                let abs = content_offset + new_expr.span.start as usize;
                let end = content_offset + new_expr.span.end as usize;
                ctx.diagnostic(
                    format!(
                        "Found a mutable instance of the built-in {} class. Use {} instead.",
                        builtin.name, builtin.svelte_name
                    ),
                    Span::new(abs as u32, end as u32),
                );
                continue;
            }

            // Check for chained method calls: `new Date().setHours(...)`.
            // The parent of NewExpression is MemberExpression, then CallExpression.
            if is_directly_mutated(nodes, new_node_id, builtin) {
                let abs = content_offset + new_expr.span.start as usize;
                let end = content_offset + new_expr.span.end as usize;
                ctx.diagnostic(
                    format!(
                        "Found a mutable instance of the built-in {} class. Use {} instead.",
                        builtin.name, builtin.svelte_name
                    ),
                    Span::new(abs as u32, end as u32),
                );
                continue;
            }

            // Walk up ancestors to find the assigned variable's SymbolId.
            // Handles both declarations (let x = new Set()) and reassignments
            // (x = new Set()).
            let (symbol_id, var_name_fallback) = find_target_symbol_or_name(nodes, scoping, new_node_id);
            let symbol_id = match symbol_id {
                Some(sid) => sid,
                None => {
                    // No symbol — e.g. implicit $: reactive declaration.
                    // Fall back to name-based mutation check.
                    // Skip $-prefixed store subscriptions — the Set/Map inside
                    // a writable store is not directly mutable.
                    if let Some(var_name) = &var_name_fallback {
                        if var_name.starts_with('$') { continue; }
                        let is_mut_by_name = is_mutated_by_name(content, var_name, builtin);
                        let is_template_mut = is_mutated_in_template(ctx.source, content, var_name, builtin);
                        if is_mut_by_name || is_template_mut {
                            let abs = content_offset + new_expr.span.start as usize;
                            let end = content_offset + new_expr.span.end as usize;
                            ctx.diagnostic(
                                format!(
                                    "Found a mutable instance of the built-in {} class. Use {} instead.",
                                    builtin.name, builtin.svelte_name
                                ),
                                Span::new(abs as u32, end as u32),
                            );
                        }
                    }
                    continue;
                }
            };

            // Only flag the first instance per mutable variable (matches
            // vendor ReferenceTracker variableStack behavior).
            if flagged_symbols.contains(&symbol_id) { continue; }

            // Check if any reference to this symbol is a mutating usage.
            let is_mut = is_symbol_mutated(scoping, nodes, symbol_id, builtin);

            // Check transitive flow: if the value flows to another variable
            // (e.g., `const today = new Date(); let viewDate = today;`)
            // and THAT variable is mutated.
            let is_transitive_mut = if !is_mut {
                is_transitively_mutated(scoping, nodes, symbol_id, builtin)
            } else {
                false
            };

            // For root-scope variables, also check template for mutations
            let is_template_mut = if !is_mut && !is_transitive_mut
                && scoping.symbol_scope_id(symbol_id) == scoping.root_scope_id()
            {
                let var_name = scoping.symbol_name(symbol_id);
                let mut found = is_mutated_in_template(ctx.source, content, var_name, builtin);
                // Also check transitive flow targets in template
                if !found {
                    for ref_id in scoping.get_resolved_references(symbol_id) {
                        if !ref_id.is_read() { continue; }
                        let (downstream_sym, _) = find_target_symbol_or_name(nodes, scoping, ref_id.node_id());
                        if let Some(ds) = downstream_sym {
                            let ds_name = scoping.symbol_name(ds);
                            if is_mutated_in_template(ctx.source, content, ds_name, builtin) {
                                found = true;
                                break;
                            }
                        }
                    }
                }
                found
            } else {
                false
            };

            if is_mut || is_transitive_mut || is_template_mut {
                flagged_symbols.insert(symbol_id);
                let abs = content_offset + new_expr.span.start as usize;
                let end = content_offset + new_expr.span.end as usize;
                ctx.diagnostic(
                    format!(
                        "Found a mutable instance of the built-in {} class. Use {} instead.",
                        builtin.name, builtin.svelte_name
                    ),
                    Span::new(abs as u32, end as u32),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a NewExpression is inside an export declaration.
fn is_inside_export(
    nodes: &oxc::semantic::AstNodes,
    node_id: oxc::semantic::NodeId,
) -> bool {
    use oxc::ast::AstKind;
    for ancestor_id in nodes.ancestor_ids(node_id) {
        match nodes.kind(ancestor_id) {
            AstKind::ExportNamedDeclaration(_) | AstKind::ExportDefaultDeclaration(_) => return true,
            AstKind::FunctionBody(_) | AstKind::ArrowFunctionExpression(_) => return false,
            _ => continue,
        }
    }
    false
}

/// Check if a NewExpression has a direct chained mutating method call,
/// e.g. `new Date().setHours(...)`, `new Set([1]).add(2)`, or
/// `(expr ?? new Set()).add(value)`.
fn is_directly_mutated(
    nodes: &oxc::semantic::AstNodes,
    new_expr_id: oxc::semantic::NodeId,
    builtin: &BuiltinClass,
) -> bool {
    use oxc::ast::AstKind;

    // Walk up through transparent wrapper nodes (LogicalExpression,
    // ParenthesizedExpression, ConditionalExpression) to find the
    // effective parent — e.g. `(expr ?? new Set()).add(value)`.
    let mut current = new_expr_id;
    loop {
        let parent_id = nodes.parent_id(current);
        match nodes.kind(parent_id) {
            AstKind::ParenthesizedExpression(_)
            | AstKind::LogicalExpression(_)
            | AstKind::ConditionalExpression(_) => {
                current = parent_id;
                continue;
            }
            _ => break,
        }
    }

    let parent_id = nodes.parent_id(current);
    let method_name = match nodes.kind(parent_id) {
        AstKind::StaticMemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    };
    if let Some(name) = method_name {
        if builtin.mutating_methods.contains(&name) {
            // Grandparent should be CallExpression (calling the method)
            let grandparent_id = nodes.parent_id(parent_id);
            if matches!(nodes.kind(grandparent_id), AstKind::CallExpression(_)) {
                return true;
            }
        }
        // For URL: check property assignment `new URL(...).pathname = ...`
        if builtin.mutating_props.contains(&name) {
            let grandparent_id = nodes.parent_id(parent_id);
            if matches!(nodes.kind(grandparent_id), AstKind::AssignmentExpression(_)) {
                return true;
            }
        }
    }
    false
}

/// Check if a NewExpression is inside a `$state(...)` call.
fn is_inside_state_call(
    nodes: &oxc::semantic::AstNodes,
    new_expr_id: oxc::semantic::NodeId,
) -> bool {
    use oxc::ast::ast::Expression;
    use oxc::ast::AstKind;

    for ancestor_id in nodes.ancestor_ids(new_expr_id) {
        match nodes.kind(ancestor_id) {
            AstKind::CallExpression(call) => {
                if let Expression::Identifier(ident) = &call.callee {
                    if matches!(ident.name.as_str(), "$state" | "$derived") {
                        return true;
                    }
                }
                return false;
            }
            AstKind::ParenthesizedExpression(_) => continue,
            _ => return false,
        }
    }
    false
}

/// Walk up ancestors from a NewExpression to find the SymbolId of the
/// assigned variable. Handles both declarations (`let x = new Set()`)
/// and reassignments (`x = new Set()`).
/// Returns (Option<SymbolId>, Option<variable_name>) — if the symbol
/// cannot be resolved (e.g. implicit `$:` declarations), the variable
/// name is returned as a fallback.
fn find_target_symbol_or_name(
    nodes: &oxc::semantic::AstNodes,
    scoping: &oxc::semantic::Scoping,
    node_id: oxc::semantic::NodeId,
) -> (Option<oxc::semantic::SymbolId>, Option<String>) {
    use oxc::ast::ast::{AssignmentTarget, BindingPattern};
    use oxc::ast::AstKind;

    for ancestor_id in nodes.ancestor_ids(node_id) {
        match nodes.kind(ancestor_id) {
            AstKind::VariableDeclarator(decl) => {
                if let BindingPattern::BindingIdentifier(ident) = &decl.id {
                    let name = ident.name.to_string();
                    return (ident.symbol_id.get(), Some(name));
                }
                return (None, None);
            }
            AstKind::AssignmentExpression(assign) => {
                if let AssignmentTarget::AssignmentTargetIdentifier(ident) = &assign.left {
                    let name = ident.name.to_string();
                    let ref_id = ident.reference_id.get();
                    let sym = ref_id.and_then(|r| scoping.get_reference(r).symbol_id());
                    return (sym, Some(name));
                }
                return (None, None);
            }
            // Pass-through nodes
            AstKind::ParenthesizedExpression(_)
            | AstKind::ExpressionStatement(_)
            | AstKind::LabeledStatement(_)
            | AstKind::CallExpression(_)
            | AstKind::LogicalExpression(_)
            | AstKind::ConditionalExpression(_)
            | AstKind::SequenceExpression(_) => continue,
            _ => return (None, None),
        }
    }
    (None, None)
}

/// Check if any read reference to the symbol is a mutating usage.
fn is_symbol_mutated(
    scoping: &oxc::semantic::Scoping,
    nodes: &oxc::semantic::AstNodes,
    symbol_id: oxc::semantic::SymbolId,
    builtin: &BuiltinClass,
) -> bool {
    use oxc::ast::AstKind;
    use oxc::span::GetSpan;

    for reference in scoping.get_resolved_references(symbol_id) {
        if !reference.is_read() { continue; }

        let parent_id = nodes.parent_id(reference.node_id());
        if let AstKind::StaticMemberExpression(member) = nodes.kind(parent_id) {
            let prop = member.property.name.as_str();

            if builtin.mutating_methods.contains(&prop) {
                let gp = nodes.parent_id(parent_id);
                if matches!(nodes.kind(gp), AstKind::CallExpression(_)) {
                    return true;
                }
            }

            if builtin.mutating_props.contains(&prop) {
                let gp = nodes.parent_id(parent_id);
                if let AstKind::AssignmentExpression(assign) = nodes.kind(gp) {
                    if member.span.start == assign.left.span().start {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check if the value of a symbol transitively flows to another variable that
/// is mutated. Handles patterns like:
///   const today = new Date(); let viewDate = currentDate ?? today; viewDate.setDate(1);
fn is_transitively_mutated(
    scoping: &oxc::semantic::Scoping,
    nodes: &oxc::semantic::AstNodes,
    symbol_id: oxc::semantic::SymbolId,
    builtin: &BuiltinClass,
) -> bool {
    use oxc::ast::AstKind;
    use oxc::ast::ast::BindingPattern;

    // For each READ reference to the symbol, check if it flows into another
    // variable (via assignment or initialization), and if THAT variable is mutated.
    for reference in scoping.get_resolved_references(symbol_id) {
        if !reference.is_read() { continue; }

        // Walk up from the reference to find if it's part of an initializer
        // for another variable (e.g., `let viewDate = currentDate ?? today`)
        let ref_node = reference.node_id();
        for ancestor_id in nodes.ancestor_ids(ref_node) {
            match nodes.kind(ancestor_id) {
                AstKind::VariableDeclarator(decl) => {
                    if let BindingPattern::BindingIdentifier(ident) = &decl.id {
                        if let Some(target_sym) = ident.symbol_id.get() {
                            if target_sym != symbol_id && is_symbol_mutated(scoping, nodes, target_sym, builtin) {
                                return true;
                            }
                        }
                    }
                    break;
                }
                AstKind::AssignmentExpression(assign) => {
                    if let oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(ident) = &assign.left {
                        let ref_id = ident.reference_id.get();
                        if let Some(target_sym) = ref_id.and_then(|r| scoping.get_reference(r).symbol_id()) {
                            if target_sym != symbol_id && is_symbol_mutated(scoping, nodes, target_sym, builtin) {
                                return true;
                            }
                        }
                    }
                    break;
                }
                // Pass through wrapper expressions
                AstKind::ParenthesizedExpression(_)
                | AstKind::LogicalExpression(_)
                | AstKind::ConditionalExpression(_)
                | AstKind::SequenceExpression(_) => continue,
                _ => break,
            }
        }
    }
    false
}

/// Check if a variable is mutated in the template (outside script blocks).
fn is_mutated_in_template(
    source: &str, script_content: &str, var_name: &str, builtin: &BuiltinClass,
) -> bool {
    for method in builtin.mutating_methods {
        let pat = format!("{}.{}(", var_name, method);
        if find_word_boundary(source, &pat) && !find_word_boundary(script_content, &pat) {
            return true;
        }
    }
    for prop in builtin.mutating_props {
        let pat = format!("{}.{} =", var_name, prop);
        if let Some(pos) = find_word_boundary_pos(source, &pat) {
            let after = &source[pos + pat.len()..];
            if !after.starts_with('=') && !find_word_boundary(script_content, &pat) {
                return true;
            }
        }
    }
    false
}

/// Check if a variable is mutated by name within the script content.
/// Used as a fallback when no SymbolId is available (e.g. implicit `$:` declarations).
fn is_mutated_by_name(content: &str, var_name: &str, builtin: &BuiltinClass) -> bool {
    for method in builtin.mutating_methods {
        let pat = format!("{}.{}(", var_name, method);
        if find_word_boundary(content, &pat) {
            return true;
        }
    }
    for prop in builtin.mutating_props {
        let pat = format!("{}.{} =", var_name, prop);
        if let Some(pos) = find_word_boundary_pos(content, &pat) {
            let after = &content[pos + pat.len()..];
            if !after.starts_with('=') {
                return true;
            }
        }
    }
    false
}

/// Find a pattern in content, ensuring it starts at a word boundary
/// (the character before the match is not alphanumeric, _, or $).
fn find_word_boundary(content: &str, pat: &str) -> bool {
    find_word_boundary_pos(content, pat).is_some()
}

fn find_word_boundary_pos(content: &str, pat: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = content[start..].find(pat) {
        let abs_pos = start + pos;
        if abs_pos == 0 {
            return Some(abs_pos);
        }
        let prev = content.as_bytes()[abs_pos - 1];
        if !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'$' {
            return Some(abs_pos);
        }
        start = abs_pos + 1;
    }
    None
}

/// Collect class names that are imported from packages (shadowing built-ins).
fn collect_shadowed_classes(content: &str) -> Vec<String> {
    let mut shadowed = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if !t.starts_with("import ") { continue; }
        if let (Some(bs), Some(be)) = (t.find('{'), t.find('}')) {
            for imp in t[bs+1..be].split(',') {
                let imp = imp.trim();
                let local = if let Some(ap) = imp.find(" as ") {
                    imp[ap + 4..].trim()
                } else { imp };
                for b in BUILTIN_CLASSES {
                    if local == b.name { shadowed.push(b.name.to_string()); }
                }
            }
        }
    }
    shadowed
}

/// Scan template (non-script) regions for `new ClassName(...).<mutatingMethod>(`.
/// This catches patterns like `{@const d = new Date(x).setHours(0)}` and
/// `{new Date().setSeconds(0, 0)}` in template expressions and event handlers.
fn detect_chained_new_in_template(ctx: &mut LintContext) {
    let source = ctx.source;

    // Collect script byte ranges to skip
    let mut script_ranges: Vec<(usize, usize)> = Vec::new();
    for script in [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten() {
        script_ranges.push((script.span.start as usize, script.span.end as usize));
    }

    // Collect shadowed class names from script imports
    let mut shadowed = Vec::new();
    for script in [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten() {
        shadowed.extend(collect_shadowed_classes(&script.content));
    }

    for builtin in BUILTIN_CLASSES {
        if shadowed.iter().any(|s| s == builtin.name) { continue; }

        let new_pat = format!("new {}(", builtin.name);
        let mut search = 0;

        while let Some(rel) = source[search..].find(&new_pat) {
            let abs = search + rel;

            // Word boundary check
            if abs > 0 {
                let prev = source.as_bytes()[abs - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' {
                    search = abs + 1;
                    continue;
                }
            }

            // Skip if inside a script block
            if script_ranges.iter().any(|&(s, e)| abs >= s && abs < e) {
                search = abs + new_pat.len();
                continue;
            }

            // Find matching `)` for the constructor call
            let paren_start = abs + new_pat.len() - 1; // position of `(`
            let close = match find_matching_paren(source, paren_start) {
                Some(p) => p,
                None => { search = abs + 1; continue; }
            };

            // Check if followed by `.mutatingMethod(`
            let after = &source[close + 1..];
            if after.starts_with('.') {
                let method_start = &after[1..];
                for method in builtin.mutating_methods {
                    let call = format!("{}(", method);
                    if method_start.starts_with(&call) {
                        ctx.diagnostic(
                            format!(
                                "Found a mutable instance of the built-in {} class. Use {} instead.",
                                builtin.name, builtin.svelte_name
                            ),
                            Span::new(abs as u32, (close + 1) as u32),
                        );
                        break;
                    }
                }
            }

            search = close + 1;
        }
    }
}

/// Find the matching closing paren for an opening paren at `pos`.
fn find_matching_paren(source: &str, pos: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(pos) != Some(&b'(') { return None; }
    let mut depth = 1i32;
    let mut i = pos + 1;
    let mut in_str = false;
    let mut sch = b'"';
    while i < bytes.len() && depth > 0 {
        if in_str {
            if bytes[i] == sch && (i == 0 || bytes[i - 1] != b'\\') { in_str = false; }
        } else {
            match bytes[i] {
                b'\'' | b'"' | b'`' => { in_str = true; sch = bytes[i]; }
                b'(' => depth += 1,
                b')' => { depth -= 1; if depth == 0 { return Some(i); } }
                _ => {}
            }
        }
        i += 1;
    }
    None
}
