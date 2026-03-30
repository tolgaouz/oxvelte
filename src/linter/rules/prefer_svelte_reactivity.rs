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

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        for script in [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten() {
            let content = &script.content;
            if content.trim().is_empty() { continue; }

            let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);
            let content_offset = script.span.start as usize + gt + 1;

            let is_ts = script.lang.as_deref() == Some("ts")
                || script.lang.as_deref() == Some("typescript");

            self.check_script(ctx, content, content_offset, is_ts);
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

            // Walk up ancestors to find the assigned variable's SymbolId.
            // Only flag declarations (VariableDeclarator), not reassignments
            // (AssignmentExpression). The vendor's iteratePropertyReferences
            // doesn't flag reassignment instances.
            let symbol_id = find_declared_symbol(nodes, new_node_id);
            let Some(symbol_id) = symbol_id else { continue };

            // Check if any reference to this symbol is a mutating usage.
            // For declarations, the vendor checks ALL references to the variable.
            let is_mut = is_symbol_mutated(scoping, nodes, symbol_id, builtin);

            // For root-scope variables, also check template for mutations
            let is_template_mut = if !is_mut
                && scoping.symbol_scope_id(symbol_id) == scoping.root_scope_id()
            {
                let var_name = scoping.symbol_name(symbol_id);
                is_mutated_in_template(ctx.source, content, var_name, builtin)
            } else {
                false
            };

            if is_mut || is_template_mut {
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
                    if ident.name.as_str() == "$state" {
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
/// variable if it's in a DECLARATION (VariableDeclarator). The vendor
/// does not flag reassignment instances (AssignmentExpression).
fn find_declared_symbol(
    nodes: &oxc::semantic::AstNodes,
    node_id: oxc::semantic::NodeId,
) -> Option<oxc::semantic::SymbolId> {
    use oxc::ast::ast::BindingPattern;
    use oxc::ast::AstKind;

    for ancestor_id in nodes.ancestor_ids(node_id) {
        match nodes.kind(ancestor_id) {
            AstKind::VariableDeclarator(decl) => {
                if let BindingPattern::BindingIdentifier(ident) = &decl.id {
                    return ident.symbol_id.get();
                }
                return None;
            }
            AstKind::AssignmentExpression(_) => return None,
            AstKind::ParenthesizedExpression(_)
            | AstKind::ExpressionStatement(_)
            | AstKind::CallExpression(_) => continue,
            _ => return None,
        }
    }
    None
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

/// Check if a variable is mutated in the template (outside script blocks).
fn is_mutated_in_template(
    source: &str, script_content: &str, var_name: &str, builtin: &BuiltinClass,
) -> bool {
    for method in builtin.mutating_methods {
        let pat = format!("{}.{}(", var_name, method);
        if source.contains(&pat) && !script_content.contains(&pat) {
            return true;
        }
    }
    for prop in builtin.mutating_props {
        let pat = format!("{}.{} =", var_name, prop);
        if let Some(pos) = source.find(&pat) {
            let after = &source[pos + pat.len()..];
            if !after.starts_with('=') && !script_content.contains(&pat) {
                return true;
            }
        }
    }
    false
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
