//! `svelte/prefer-const` — require `const` declarations for variables that are
//! never reassigned. 🔧 Fixable.
//!
//! Mirrors ESLint core's `prefer-const`, with the Svelte-specific override:
//! a declaration whose initializer is one of the configured `excludedRunes`
//! (`$props`, `$derived` by default) is skipped *as a whole declaration*,
//! not per-symbol.
//!
//! Implementation notes:
//! - Walks `AstKind::VariableDeclaration` so we can reason about every
//!   declarator in a single `let`.
//! - Symbol resolution uses `BindingIdentifier::symbol_id`. References are
//!   read off the scope-manager's `get_resolved_references`.
//! - Template writes (`bind:value={x}`, `onclick={() => x = 1}`) are merged
//!   into the instance-script analysis because Svelte's parser exposes those
//!   as writes to the same component-scope binding.
//! - The autofix replaces the `let` keyword with `const`, but only when *every*
//!   binding in the whole declaration is eligible (matching ESLint core).
//! - `ignoreReadBeforeAssign` is currently a no-op — implementing it
//!   faithfully needs read/write-ordering analysis we don't have yet.

use crate::ast::{
    Attribute, AttributeValue, AttributeValuePart, DirectiveKind, Fragment, TemplateNode,
};
use crate::linter::walk_template_nodes;
use crate::linter::{Fix, LintContext, Rule};
use oxc::ast::ast::{
    ArrayAssignmentTarget, AssignmentTarget, AssignmentTargetMaybeDefault,
    AssignmentTargetProperty, BindingPattern, Expression, IdentifierReference, ModuleExportName,
    ObjectAssignmentTarget, SimpleAssignmentTarget, VariableDeclarationKind,
};
use oxc::ast::AstKind;
use oxc::semantic::{AstNodes, NodeId, Scoping, Semantic, SymbolId};
use oxc::span::Span;
use std::collections::HashSet;

pub struct PreferConst;

#[derive(Clone)]
struct Finding {
    message: String,
    span: Span,
    fix: Option<Fix>,
}

#[derive(Default)]
struct SemanticWrites {
    symbols: HashSet<SymbolId>,
    unresolved_names: HashSet<String>,
}

struct CollectFacts<'a> {
    template_writes: &'a HashSet<String>,
    external_writes: &'a HashSet<String>,
    semantic_writes: &'a HashSet<SymbolId>,
    exported_symbols: &'a HashSet<SymbolId>,
    skip_exported_props: bool,
}

impl Rule for PreferConst {
    fn name(&self) -> &'static str {
        "svelte/prefer-const"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let excluded_runes: Vec<String> = opts
            .and_then(|o| o.get("excludedRunes"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["$props".into(), "$derived".into()]);
        let destructuring_all = opts
            .and_then(|o| o.get("destructuring"))
            .and_then(|v| v.as_str())
            == Some("all");

        let mut findings: Vec<Finding> = Vec::new();
        let template_writes = collect_template_writes(ctx);
        collect_template_findings(ctx, &excluded_runes, destructuring_all, &mut findings);

        let instance_writes = ctx
            .instance_semantic
            .map(collect_semantic_writes)
            .unwrap_or_default();
        let module_writes = ctx
            .module_semantic
            .map(collect_semantic_writes)
            .unwrap_or_default();
        let mut module_external_writes = instance_writes.unresolved_names.clone();
        module_external_writes.extend(template_writes.iter().cloned());

        if let Some(s) = ctx.instance_semantic {
            let exported_symbols = collect_exported_specifier_symbols(s);
            let external_writes = HashSet::new();
            collect(
                s,
                ctx.instance_content_offset,
                &excluded_runes,
                destructuring_all,
                CollectFacts {
                    template_writes: &template_writes,
                    external_writes: &external_writes,
                    semantic_writes: &instance_writes.symbols,
                    exported_symbols: &exported_symbols,
                    skip_exported_props: true,
                },
                &mut findings,
            );
        }
        if let Some(s) = ctx.module_semantic {
            let exported_symbols = collect_exported_specifier_symbols(s);
            let template_writes = HashSet::new();
            collect(
                s,
                ctx.module_content_offset,
                &excluded_runes,
                destructuring_all,
                CollectFacts {
                    template_writes: &template_writes,
                    external_writes: &module_external_writes,
                    semantic_writes: &module_writes.symbols,
                    exported_symbols: &exported_symbols,
                    skip_exported_props: false,
                },
                &mut findings,
            );
        }
        for f in findings {
            match f.fix {
                Some(fix) => ctx.diagnostic_with_fix(f.message, f.span, fix),
                None => ctx.diagnostic(f.message, f.span),
            }
        }
    }
}

fn collect<'a>(
    semantic: &Semantic<'a>,
    content_offset: u32,
    excluded_runes: &[String],
    destructuring_all: bool,
    facts: CollectFacts<'_>,
    findings: &mut Vec<Finding>,
) {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    for node in semantic.nodes().iter() {
        let AstKind::VariableDeclaration(vd) = node.kind() else {
            continue;
        };
        if vd.kind != VariableDeclarationKind::Let {
            continue;
        }
        if facts.skip_exported_props && is_inside_export_named_decl(nodes, node.id()) {
            continue;
        }

        // Vendor's whole-declaration short-circuit: if any declarator's init is
        // a rune from `excludedRunes`, the entire declaration is skipped —
        // not just the affected declarator.
        let any_excluded = vd.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .and_then(rune_name)
                .is_some_and(|rune| excluded_runes.iter().any(|r| r == rune))
        });
        if any_excluded {
            continue;
        }

        // For each declarator, examine the bindings it introduces.
        let mut per_declarator: Vec<Vec<(String, Span)>> = Vec::new();
        let mut total_bindings = 0;
        let mut total_eligible = 0;
        for decl in &vd.declarations {
            // Skip declarators with no initializer — `let x;` cannot be
            // converted to `const`.
            if decl.init.is_none() {
                per_declarator.push(Vec::new());
                continue;
            }
            let mut bindings: Vec<(SymbolId, String, Span)> = Vec::new();
            collect_pattern_bindings(&decl.id, &mut bindings);
            total_bindings += bindings.len();

            let eligible: Vec<(String, Span)> = bindings
                .iter()
                .filter(|(sid, name, _)| {
                    !facts.exported_symbols.contains(sid)
                        && !facts.semantic_writes.contains(sid)
                        && !facts.template_writes.contains(name)
                        && !facts.external_writes.contains(name)
                        && !scoping.get_resolved_references(*sid).any(|r| r.is_write())
                })
                .map(|(_, name, span)| (name.clone(), *span))
                .collect();
            total_eligible += eligible.len();

            // `destructuring: 'all'` requires every binding in the *pattern*
            // to be eligible before we report. `'any'` (default) reports
            // whatever's eligible.
            let pattern_passes = if destructuring_all && bindings.len() > 1 {
                eligible.len() == bindings.len()
            } else {
                !eligible.is_empty()
            };
            per_declarator.push(if pattern_passes { eligible } else { Vec::new() });
        }

        let any_to_report = per_declarator.iter().any(|v| !v.is_empty());
        if !any_to_report {
            continue;
        }

        // Autofix only when every binding in the whole declaration is
        // eligible (matches ESLint core's behavior — partial declarations
        // would leave the keyword inconsistent with the un-fixed members).
        let whole_declaration_eligible = total_bindings > 0 && total_eligible == total_bindings;
        let let_keyword_span = Span::new(
            content_offset + vd.span.start,
            content_offset + vd.span.start + 3,
        );

        for eligible in &per_declarator {
            for (name, binding_span) in eligible {
                let abs_span = Span::new(
                    content_offset + binding_span.start,
                    content_offset + binding_span.end,
                );
                let message = format!("'{}' is never reassigned. Use 'const' instead.", name);
                let fix = whole_declaration_eligible.then(|| Fix {
                    span: let_keyword_span,
                    replacement: "const".into(),
                });
                findings.push(Finding {
                    message,
                    span: abs_span,
                    fix,
                });
            }
        }
    }
}

fn is_inside_export_named_decl(nodes: &AstNodes, node_id: NodeId) -> bool {
    nodes
        .ancestor_ids(node_id)
        .any(|ancestor| matches!(nodes.kind(ancestor), AstKind::ExportNamedDeclaration(_)))
}

fn collect_exported_specifier_symbols<'a>(semantic: &'a Semantic<'a>) -> HashSet<SymbolId> {
    let scoping = semantic.scoping();
    let mut exported = HashSet::new();
    for node in semantic.nodes().iter() {
        let AstKind::ExportNamedDeclaration(exp) = node.kind() else {
            continue;
        };
        if exp.declaration.is_some() {
            continue;
        }
        for spec in &exp.specifiers {
            let ModuleExportName::IdentifierReference(local) = &spec.local else {
                continue;
            };
            let Some(reference_id) = local.reference_id.get() else {
                continue;
            };
            if let Some(symbol_id) = scoping.get_reference(reference_id).symbol_id() {
                exported.insert(symbol_id);
            }
        }
    }
    exported
}

fn collect_template_findings<'a>(
    ctx: &LintContext<'a>,
    excluded_runes: &[String],
    destructuring_all: bool,
    findings: &mut Vec<Finding>,
) {
    visit_template_expressions(&ctx.ast.html, &mut |text, expression_span| {
        collect_expression_findings(
            text,
            expression_span.start,
            excluded_runes,
            destructuring_all,
            findings,
        );
    });
}

fn collect_expression_findings(
    text: &str,
    text_start: u32,
    excluded_runes: &[String],
    destructuring_all: bool,
    findings: &mut Vec<Finding>,
) {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::semantic::SemanticBuilder;
    use oxc::span::SourceType;

    let alloc = Allocator::default();
    let wrapper = format!("({});", text);
    let parsed = Parser::new(&alloc, &wrapper, SourceType::ts()).parse();
    if !parsed.errors.is_empty() {
        return;
    }
    let semantic = SemanticBuilder::new().build(&parsed.program).semantic;
    collect(
        &semantic,
        text_start.saturating_sub(1),
        excluded_runes,
        destructuring_all,
        CollectFacts {
            template_writes: &HashSet::new(),
            external_writes: &HashSet::new(),
            semantic_writes: &HashSet::new(),
            exported_symbols: &HashSet::new(),
            skip_exported_props: false,
        },
        findings,
    );
}

fn collect_semantic_writes(semantic: &Semantic<'_>) -> SemanticWrites {
    let mut writes = SemanticWrites::default();
    let scoping = semantic.scoping();
    for node in semantic.nodes().iter() {
        match node.kind() {
            AstKind::AssignmentExpression(ae) => {
                collect_assignment_target_write_facts(&ae.left, scoping, &mut writes);
            }
            AstKind::UpdateExpression(ue) => {
                if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &ue.argument {
                    collect_write_fact(id, scoping, &mut writes);
                }
            }
            _ => {}
        }
    }
    writes
}

fn collect_template_writes(ctx: &LintContext<'_>) -> HashSet<String> {
    let mut writes = HashSet::new();
    walk_template_nodes(&ctx.ast.html, &mut |node| {
        if let TemplateNode::Element(el) = node {
            for (idx, attr) in el.attributes.iter().enumerate() {
                if let Attribute::Directive {
                    kind: DirectiveKind::Binding,
                    name,
                    value,
                    ..
                } = attr
                {
                    match value {
                        AttributeValue::True if is_identifier_text(name) => {
                            writes.insert(name.clone());
                        }
                        AttributeValue::Expression(text) => {
                            if let Some(name) = el
                                .attribute_expression_ast(idx)
                                .and_then(bare_identifier_expr)
                                .or_else(|| {
                                    let trimmed = text.trim();
                                    is_identifier_text(trimmed).then_some(trimmed)
                                })
                            {
                                writes.insert(name.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    visit_template_expressions(&ctx.ast.html, &mut |text, _| {
        collect_expression_writes(text, &mut writes);
    });
    writes
}

fn visit_template_expressions(fragment: &Fragment, visitor: &mut impl FnMut(&str, Span)) {
    walk_template_nodes(fragment, &mut |node| match node {
        TemplateNode::Element(el) => {
            for (idx, attr) in el.attributes.iter().enumerate() {
                match attr {
                    Attribute::NormalAttribute { value, .. }
                    | Attribute::Directive { value, .. } => match value {
                        AttributeValue::Expression(text) => {
                            if let Some(span) =
                                el.attribute_meta.get(idx).and_then(|m| m.expression_span)
                            {
                                visitor(text.as_str(), span);
                            }
                        }
                        AttributeValue::Concat(parts) => {
                            for (part_idx, part) in parts.iter().enumerate() {
                                let AttributeValuePart::Expression(text) = part else {
                                    continue;
                                };
                                let Some(span) = el
                                    .attribute_meta
                                    .get(idx)
                                    .and_then(|m| m.parts.get(part_idx))
                                    .and_then(|p| p.expression_span)
                                else {
                                    continue;
                                };
                                visitor(text.as_str(), span);
                            }
                        }
                        _ => {}
                    },
                    Attribute::Spread { .. } => {}
                }
            }
        }
        TemplateNode::MustacheTag(tag) => visitor(tag.expression.as_str(), tag.expression_span),
        TemplateNode::RawMustacheTag(tag) => visitor(tag.expression.as_str(), tag.expression_span),
        TemplateNode::RenderTag(tag) => visitor(tag.expression.as_str(), tag.expression_span),
        TemplateNode::IfBlock(block) => visitor(block.test.as_str(), block.test_span),
        TemplateNode::EachBlock(block) => {
            visitor(block.expression.as_str(), block.expression_span);
            if let Some(span) = block.key_span {
                if let Some(key) = &block.key {
                    visitor(key.as_str(), span);
                }
            }
        }
        TemplateNode::AwaitBlock(block) => {
            visitor(block.expression.as_str(), block.expression_span)
        }
        TemplateNode::KeyBlock(block) => visitor(block.expression.as_str(), block.expression_span),
        TemplateNode::Text(_)
        | TemplateNode::DebugTag(_)
        | TemplateNode::ConstTag(_)
        | TemplateNode::SnippetBlock(_)
        | TemplateNode::Comment(_) => {}
    });
}

fn collect_expression_writes(text: &str, writes: &mut HashSet<String>) {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::semantic::SemanticBuilder;
    use oxc::span::SourceType;

    let alloc = Allocator::default();
    let wrapper = format!("({});", text);
    let parsed = Parser::new(&alloc, &wrapper, SourceType::ts()).parse();
    if !parsed.errors.is_empty() {
        return;
    }
    let semantic = SemanticBuilder::new().build(&parsed.program).semantic;
    let scoping = semantic.scoping();

    for node in semantic.nodes().iter() {
        match node.kind() {
            AstKind::AssignmentExpression(ae) => {
                collect_assignment_target_writes(&ae.left, scoping, writes);
            }
            AstKind::UpdateExpression(ue) => {
                if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &ue.argument {
                    collect_unresolved_write(id, scoping, writes);
                }
            }
            _ => {}
        }
    }
}

fn collect_assignment_target_writes<'a>(
    target: &'a AssignmentTarget<'a>,
    scoping: &Scoping,
    writes: &mut HashSet<String>,
) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            collect_unresolved_write(id, scoping, writes);
        }
        AssignmentTarget::ObjectAssignmentTarget(obj) => {
            collect_object_target_writes(obj, scoping, writes);
        }
        AssignmentTarget::ArrayAssignmentTarget(arr) => {
            collect_array_target_writes(arr, scoping, writes);
        }
        _ => {}
    }
}

fn collect_assignment_target_write_facts<'a>(
    target: &'a AssignmentTarget<'a>,
    scoping: &Scoping,
    writes: &mut SemanticWrites,
) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            collect_write_fact(id, scoping, writes);
        }
        AssignmentTarget::ObjectAssignmentTarget(obj) => {
            collect_object_target_write_facts(obj, scoping, writes);
        }
        AssignmentTarget::ArrayAssignmentTarget(arr) => {
            collect_array_target_write_facts(arr, scoping, writes);
        }
        _ => {}
    }
}

fn collect_object_target_writes<'a>(
    obj: &'a ObjectAssignmentTarget<'a>,
    scoping: &Scoping,
    writes: &mut HashSet<String>,
) {
    for prop in &obj.properties {
        match prop {
            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                collect_unresolved_write(&p.binding, scoping, writes);
            }
            AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                collect_maybe_default_writes(&p.binding, scoping, writes);
            }
        }
    }
    if let Some(rest) = &obj.rest {
        collect_assignment_target_writes(&rest.target, scoping, writes);
    }
}

fn collect_object_target_write_facts<'a>(
    obj: &'a ObjectAssignmentTarget<'a>,
    scoping: &Scoping,
    writes: &mut SemanticWrites,
) {
    for prop in &obj.properties {
        match prop {
            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                collect_write_fact(&p.binding, scoping, writes);
            }
            AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                collect_maybe_default_write_facts(&p.binding, scoping, writes);
            }
        }
    }
    if let Some(rest) = &obj.rest {
        collect_assignment_target_write_facts(&rest.target, scoping, writes);
    }
}

fn collect_array_target_writes<'a>(
    arr: &'a ArrayAssignmentTarget<'a>,
    scoping: &Scoping,
    writes: &mut HashSet<String>,
) {
    for el in arr.elements.iter().flatten() {
        collect_maybe_default_writes(el, scoping, writes);
    }
    if let Some(rest) = &arr.rest {
        collect_assignment_target_writes(&rest.target, scoping, writes);
    }
}

fn collect_array_target_write_facts<'a>(
    arr: &'a ArrayAssignmentTarget<'a>,
    scoping: &Scoping,
    writes: &mut SemanticWrites,
) {
    for el in arr.elements.iter().flatten() {
        collect_maybe_default_write_facts(el, scoping, writes);
    }
    if let Some(rest) = &arr.rest {
        collect_assignment_target_write_facts(&rest.target, scoping, writes);
    }
}

fn collect_maybe_default_writes<'a>(
    target: &'a AssignmentTargetMaybeDefault<'a>,
    scoping: &Scoping,
    writes: &mut HashSet<String>,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
            collect_assignment_target_writes(&with_default.binding, scoping, writes);
        }
        AssignmentTargetMaybeDefault::AssignmentTargetIdentifier(id) => {
            collect_unresolved_write(id, scoping, writes);
        }
        AssignmentTargetMaybeDefault::ObjectAssignmentTarget(obj) => {
            collect_object_target_writes(obj, scoping, writes);
        }
        AssignmentTargetMaybeDefault::ArrayAssignmentTarget(arr) => {
            collect_array_target_writes(arr, scoping, writes);
        }
        _ => {}
    }
}

fn collect_maybe_default_write_facts<'a>(
    target: &'a AssignmentTargetMaybeDefault<'a>,
    scoping: &Scoping,
    writes: &mut SemanticWrites,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
            collect_assignment_target_write_facts(&with_default.binding, scoping, writes);
        }
        AssignmentTargetMaybeDefault::AssignmentTargetIdentifier(id) => {
            collect_write_fact(id, scoping, writes);
        }
        AssignmentTargetMaybeDefault::ObjectAssignmentTarget(obj) => {
            collect_object_target_write_facts(obj, scoping, writes);
        }
        AssignmentTargetMaybeDefault::ArrayAssignmentTarget(arr) => {
            collect_array_target_write_facts(arr, scoping, writes);
        }
        _ => {}
    }
}

fn collect_unresolved_write(
    id: &IdentifierReference<'_>,
    scoping: &Scoping,
    writes: &mut HashSet<String>,
) {
    if scoping
        .get_reference(id.reference_id())
        .symbol_id()
        .is_none()
    {
        writes.insert(id.name.to_string());
    }
}

fn collect_write_fact(
    id: &IdentifierReference<'_>,
    scoping: &Scoping,
    writes: &mut SemanticWrites,
) {
    match scoping.get_reference(id.reference_id()).symbol_id() {
        Some(symbol_id) => {
            writes.symbols.insert(symbol_id);
        }
        None => {
            writes.unresolved_names.insert(id.name.to_string());
        }
    }
}

fn bare_identifier_expr<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn is_identifier_text(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

/// Append every `BindingIdentifier` reachable inside `pat` to `out`, along
/// with its resolved `SymbolId` and source-span.
fn collect_pattern_bindings<'a>(pat: &BindingPattern<'a>, out: &mut Vec<(SymbolId, String, Span)>) {
    match pat {
        BindingPattern::BindingIdentifier(id) => {
            if let Some(sid) = id.symbol_id.get() {
                out.push((sid, id.name.to_string(), id.span));
            }
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_bindings(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_bindings(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_pattern_bindings(el, out);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_bindings(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(inner) => {
            collect_pattern_bindings(&inner.left, out);
        }
    }
}

/// For expressions like `$state(...)`, `$props()`, `$derived(...)`, or the
/// shorthand `$derived` / `$props.id` accesses, return the leading `$foo` name.
fn rune_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::CallExpression(ce) => match &ce.callee {
            Expression::Identifier(id) if id.name.starts_with('$') => Some(id.name.as_str()),
            Expression::StaticMemberExpression(mem) => {
                if let Expression::Identifier(id) = &mem.object {
                    if id.name.starts_with('$') {
                        return Some(id.name.as_str());
                    }
                }
                None
            }
            _ => None,
        },
        Expression::Identifier(id) if id.name.starts_with('$') => Some(id.name.as_str()),
        Expression::StaticMemberExpression(mem) => {
            if let Expression::Identifier(id) = &mem.object {
                if id.name.starts_with('$') {
                    return Some(id.name.as_str());
                }
            }
            None
        }
        _ => None,
    }
}
