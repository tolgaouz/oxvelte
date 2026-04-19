//! `svelte/no-reactive-reassign` — disallow reassignment of reactive values.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use oxc::ast::ast::{
    ArrayAssignmentTarget, AssignmentTarget, AssignmentTargetMaybeDefault,
    AssignmentTargetProperty, Declaration, Expression, ForStatementLeft, IdentifierReference,
    ImportDeclarationSpecifier, ObjectAssignmentTarget, SimpleAssignmentTarget, Statement,
    VariableDeclaration,
};
use oxc::ast::AstKind;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::Span;
use std::collections::HashSet;

pub struct NoReactiveReassign;

const MUTATING_METHODS: &[&str] = &[
    "push(", "pop(", "shift(", "unshift(", "splice(",
    "sort(", "reverse(", "fill(", "copyWithin(",
];

impl Rule for NoReactiveReassign {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-reassign"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let check_props = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("props"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let Some(script) = &ctx.ast.instance else { return };
        let content = &script.content;
        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        let Some(semantic) = ctx.instance_semantic else { return };

        let (mut reactive_vars, declared_names) = collect_reactive_and_declared(semantic);
        reactive_vars.retain(|v| !declared_names.contains(v));
        if reactive_vars.is_empty() { return; }

        // Direct identifier reassignment: `foo = ...`, `foo += ...`, `foo++`, etc.
        // Flagged when the target is not resolvable to a local (shadowed) binding and
        // not inside a `$: foo = ...` declaration itself.
        let scoping = semantic.scoping();
        let nodes = semantic.nodes();
        for node in nodes.iter() {
            match node.kind() {
                AstKind::AssignmentExpression(ae) => {
                    if let AssignmentTarget::AssignmentTargetIdentifier(id) = &ae.left {
                        let name = id.name.as_str();
                        if !reactive_vars.contains(name) { continue; }
                        if scoping.get_reference(id.reference_id()).symbol_id().is_some() { continue; }
                        if is_direct_reactive_decl(nodes, node.id()) { continue; }
                        let sp = content_offset as u32 + id.span.start;
                        let end = content_offset as u32 + ae.span.end.min(span_after_left_op(ae));
                        ctx.diagnostic(
                            format!("Assignment to reactive value '{}'.", name),
                            Span::new(sp, end),
                        );
                    }
                }
                AstKind::UpdateExpression(ue) => {
                    if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &ue.argument {
                        let name = id.name.as_str();
                        if !reactive_vars.contains(name) { continue; }
                        if scoping.get_reference(id.reference_id()).symbol_id().is_some() { continue; }
                        if is_direct_reactive_decl(nodes, node.id()) { continue; }
                        let sp = content_offset as u32 + ue.span.start;
                        let end = content_offset as u32 + ue.span.end;
                        ctx.diagnostic(
                            format!("Assignment to reactive value '{}'.", name),
                            Span::new(sp, end),
                        );
                    }
                }
                _ => {}
            }
        }

        if check_props {
            // Flag reassignments that reach a reactive var via a member chain:
            // `var.prop = ...`, `var[x] = ...`, `var.a.b++`, `--var.p`,
            // `var.push(...)`, `var.a.sort(...)`, `delete var.prop`.
            //
            // The direct-identifier forms (`var = x`, `var++`) are already
            // handled by the `reactive_vars` walk above. Here we only look at
            // property-reaching expressions, so `depth == 0` is skipped for
            // assignment/update targets (it's a duplicate of the direct walk).
            // For call expressions, `depth == 0` corresponds to `var.push()`,
            // which is the vendor's "Assignment to reactive value" case (not
            // property), so we distinguish via the message.
            const MUTATING_NAMES: &[&str] = &[
                "push", "pop", "shift", "unshift", "splice",
                "sort", "reverse", "fill", "copyWithin",
            ];
            for node in nodes.iter() {
                let (base, depth, span_end, is_method_call) = match node.kind() {
                    AstKind::AssignmentExpression(ae) => {
                        let Some((b, d)) = target_member_path(&ae.left) else { continue };
                        if d == 0 { continue; }
                        (b, d, ae.span.end, false)
                    }
                    AstKind::UpdateExpression(ue) => {
                        let Some((b, d)) = simple_target_member_path(&ue.argument) else { continue };
                        if d == 0 { continue; }
                        (b, d, ue.span.end, false)
                    }
                    AstKind::CallExpression(ce) => {
                        let (method_name, base_expr) = match &ce.callee {
                            Expression::StaticMemberExpression(m) => (m.property.name.as_str(), &m.object),
                            Expression::ChainExpression(c) => match &c.expression {
                                oxc::ast::ast::ChainElement::StaticMemberExpression(m) =>
                                    (m.property.name.as_str(), &m.object),
                                _ => continue,
                            },
                            _ => continue,
                        };
                        if !MUTATING_NAMES.contains(&method_name) { continue; }
                        let Some((b, d)) = expr_member_path(base_expr) else { continue };
                        (b, d, ce.span.end, true)
                    }
                    AstKind::UnaryExpression(ue)
                        if ue.operator == oxc::syntax::operator::UnaryOperator::Delete =>
                    {
                        let Some(b) = expr_base_ident(&ue.argument) else { continue };
                        // Always reported as "property of" in the old rule, so
                        // force depth >= 1 here regardless of actual depth.
                        (b, 1, ue.span.end, false)
                    }
                    _ => continue,
                };
                let base_name = base.name.as_str();
                // A `$`-prefixed reference is Svelte's auto-subscription to the
                // store/reactive value bound under the unprefixed name — treat
                // `$likes.x = …` as a write against the reactive `likes`.
                let is_reactive_ref = reactive_vars.contains(base_name)
                    || (base_name.starts_with('$') && reactive_vars.contains(&base_name[1..]));
                if !is_reactive_ref { continue; }
                if scoping.get_reference(base.reference_id()).symbol_id().is_some() { continue; }
                if is_in_direct_reactive_statement(nodes, node.id()) { continue; }
                let sp = content_offset as u32 + base.span.start;
                let end = content_offset as u32 + span_end;
                let msg = if depth == 0 && is_method_call {
                    format!("Assignment to reactive value '{}'.", base_name)
                } else {
                    format!("Assignment to property of reactive value '{}'.", base_name)
                };
                ctx.diagnostic(msg, Span::new(sp, end));
            }
        } // end if check_props

        // Destructure reassignments: `({ reactiveVar } = x)`, `([reactiveVar] = x)`,
        // `([foo, ...reactiveVar] = x)`, nested array/object patterns, and
        // renamed property destructure `({ name: reactiveVar } = x)`. `const`
        // / `let` / `var` declarations are ignored because those are
        // VariableDeclarator bindings — not AssignmentExpression — so they
        // never reach this walk.
        for node in nodes.iter() {
            let AstKind::AssignmentExpression(ae) = node.kind() else { continue };
            if !matches!(&ae.left,
                AssignmentTarget::ObjectAssignmentTarget(_)
                | AssignmentTarget::ArrayAssignmentTarget(_)
            ) { continue; }
            if is_in_direct_reactive_statement(nodes, node.id()) { continue; }
            let mut idents = Vec::new();
            collect_target_idents(&ae.left, &mut idents);
            let mut reported = std::collections::HashSet::new();
            for id in &idents {
                let name = id.name.as_str();
                let is_reactive = reactive_vars.contains(name)
                    || (name.starts_with('$') && reactive_vars.contains(&name[1..]));
                if !is_reactive { continue; }
                if scoping.get_reference(id.reference_id()).symbol_id().is_some() { continue; }
                if !reported.insert(name) { continue; } // report each var once per pattern
                let sp = content_offset as u32 + id.span.start;
                let end = content_offset as u32 + id.span.end;
                ctx.diagnostic(
                    format!("Assignment to reactive value '{}'.", name),
                    Span::new(sp, end),
                );
            }
        }

        // Detect `for (reactiveVar of/in ...)` and `for (reactiveVar.p of/in ...)`
        // on the LHS of a for-in / for-of statement via the AST. `for (const x ...)` /
        // `for (let x ...)` declare new local bindings, not reassignments.
        for node in nodes.iter() {
            let (for_span, left) = match node.kind() {
                AstKind::ForInStatement(f) => (f.span, &f.left),
                AstKind::ForOfStatement(f) => (f.span, &f.left),
                _ => continue,
            };
            let (name, name_end, is_prop) = match left {
                ForStatementLeft::VariableDeclaration(_) => continue,
                ForStatementLeft::AssignmentTargetIdentifier(id) => {
                    if scoping.get_reference(id.reference_id()).symbol_id().is_some() { continue; }
                    (id.name.as_str(), id.span.end, false)
                }
                ForStatementLeft::StaticMemberExpression(m) => match expr_base_ident(&m.object) {
                    Some(id) if scoping.get_reference(id.reference_id()).symbol_id().is_none() =>
                        (id.name.as_str(), id.span.end, true),
                    _ => continue,
                },
                ForStatementLeft::ComputedMemberExpression(m) => match expr_base_ident(&m.object) {
                    Some(id) if scoping.get_reference(id.reference_id()).symbol_id().is_none() =>
                        (id.name.as_str(), id.span.end, true),
                    _ => continue,
                },
                ForStatementLeft::PrivateFieldExpression(m) => match expr_base_ident(&m.object) {
                    Some(id) if scoping.get_reference(id.reference_id()).symbol_id().is_none() =>
                        (id.name.as_str(), id.span.end, true),
                    _ => continue,
                },
                _ => continue,
            };
            if !reactive_vars.contains(name) { continue; }
            if is_prop && !check_props { continue; }
            let sp = content_offset as u32 + for_span.start;
            let end = content_offset as u32 + name_end;
            let msg = if is_prop {
                format!("Assignment to property of reactive value '{}'.", name)
            } else {
                format!("Assignment to reactive value '{}'.", name)
            };
            ctx.diagnostic(msg, Span::new(sp, end));
        }

        // Conditional member assignment: `(cond ? reactiveVar : x).prop = y`
        // and nested ternaries. The property-mutation walk above only resolves
        // a member expression whose `.object` is directly an identifier or a
        // nested member chain; going through a ConditionalExpression branch
        // needs a separate check.
        if check_props {
            for node in nodes.iter() {
                let AstKind::AssignmentExpression(ae) = node.kind() else { continue };
                let member_object = match &ae.left {
                    AssignmentTarget::StaticMemberExpression(m) => &m.object,
                    AssignmentTarget::ComputedMemberExpression(m) => &m.object,
                    AssignmentTarget::PrivateFieldExpression(m) => &m.object,
                    _ => continue,
                };
                let Some(id) = find_reactive_via_conditional(member_object, &reactive_vars) else { continue };
                if scoping.get_reference(id.reference_id()).symbol_id().is_some() { continue; }
                if is_in_direct_reactive_statement(nodes, node.id()) { continue; }
                let sp = content_offset as u32 + ae.span.start;
                let end = content_offset as u32 + ae.span.end;
                ctx.diagnostic(
                    format!("Assignment to property of reactive value '{}'.", id.name),
                    Span::new(sp, end),
                );
            }
        }

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    let expr_span = match attr {
                        Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } => Some(*span),
                        Attribute::NormalAttribute { span, value, .. } => {
                            match value {
                                crate::ast::AttributeValue::Expression(_) => Some(*span),
                                crate::ast::AttributeValue::Concat(_) => Some(*span),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(span) = expr_span {
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        let tmpl_suffixes: &[&str] = &[" = ", " += ", " -= ", " *= ", " /= ", " %= ", "++", "--"];
                        for var in &reactive_vars {
                            let pats: Vec<String> = tmpl_suffixes.iter().map(|s| format!("{}{}", var, s)).collect();
                            'next_var: for pat in &pats {
                                for (pos, _) in region.match_indices(pat.as_str()) {
                                    if pos > 0 && matches!(region.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'.') { continue; }
                                    if pat.ends_with("= ") && pos + pat.len() - 1 < region.len() && region.as_bytes()[pos + pat.len() - 1] == b'=' { continue; }
                                    let before = &region[..pos];
                                    if before.matches('\'').count() % 2 != 0 || before.matches('"').count() % 2 != 0 { continue; }
                                    let ap = span.start as usize + pos;
                                    ctx.diagnostic(format!("Assignment to reactive value '{}'.", var),
                                        oxc::span::Span::new(ap as u32, (ap + var.len()) as u32));
                                    break 'next_var;
                                }
                            }
                        }
                        if check_props { for var in &reactive_vars {
                            for prefix in &[var.clone(), format!("${}", var)] {
                                for pat_start in &[format!("{}.", prefix), format!("{}[", prefix)] {
                                    for (pos, _) in region.match_indices(pat_start.as_str()) {
                                        if pos > 0 && matches!(region.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') { continue; }
                                        let before = &region[..pos];
                                        if before.matches('\'').count() % 2 != 0 || before.matches('"').count() % 2 != 0 { continue; }
                                        let after = &region[pos + pat_start.len()..];
                                        let mut rest = if pat_start.ends_with('[') {
                                            after.find(']').map(|p| &after[p+1..]).unwrap_or("")
                                        } else {
                                            let end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
                                            &after[end..]
                                        };
                                        loop {
                                            if rest.starts_with('.') || rest.starts_with("?.") {
                                                let skip = if rest.starts_with("?.") { 2 } else { 1 };
                                                let r = &rest[skip..];
                                                for m in MUTATING_METHODS {
                                                    if r.starts_with(*m) {
                                                        let ap = span.start as usize + pos;
                                                        ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", prefix),
                                                            oxc::span::Span::new(ap as u32, (ap + pat_start.len()) as u32));
                                                    }
                                                }
                                                let end = r.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(r.len());
                                                rest = &r[end..];
                                            } else if rest.starts_with('[') {
                                                rest = rest[1..].find(']').map(|p| &rest[p+2..]).unwrap_or("");
                                            } else { break; }
                                        }
                                        let rest = rest.trim_start();
                                        if rest.starts_with('=') && !rest.starts_with("==") {
                                            let ap = span.start as usize + pos;
                                            ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", prefix),
                                                oxc::span::Span::new(ap as u32, (ap + pat_start.len()) as u32));
                                            break;
                                        }
                                    }
                                }
                            }
                        }}
                    }
                    if let Attribute::Directive { kind: DirectiveKind::Binding, name, span, .. } = attr {
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let (Some(open), Some(close)) = (region.find('{'), region.find('}')) {
                            let bound = region[open+1..close].trim();
                            let base = bound.split('.').next().unwrap_or(bound);
                            if reactive_vars.contains(bound) || (reactive_vars.contains(base) && (check_props || !bound.contains('.'))) {
                                ctx.diagnostic(format!("Assignment to reactive value '{}'.", base), *span);
                            }
                        } else if reactive_vars.contains(name.as_str()) {
                            ctx.diagnostic(format!("Assignment to reactive value '{}'.", name), *span);
                        }
                    }
                }
            }
        });
    }
}

/// Walk the top level of the instance script. Returns `(reactive_vars, declared_names)`:
/// - `reactive_vars`: names introduced as the LHS of `$: name = expr` labeled statements.
/// - `declared_names`: names bound by any other top-level declaration
///   (`let`/`const`/`var`/`function`/`class`/`import`/TS type decls), used to
///   remove names that are *both* declared and reactively assigned (in which
///   case the `$:` isn't declaring a reactive var, it's updating a regular
///   binding).
fn collect_reactive_and_declared(semantic: &Semantic<'_>) -> (HashSet<String>, HashSet<String>) {
    let mut reactive = HashSet::new();
    let mut declared = HashSet::new();
    for stmt in &semantic.nodes().program().body {
        match stmt {
            Statement::LabeledStatement(ls) if ls.label.name == "$" => {
                let Statement::ExpressionStatement(es) = &ls.body else { continue };
                let Expression::AssignmentExpression(ae) = &es.expression else { continue };
                if let AssignmentTarget::AssignmentTargetIdentifier(id) = &ae.left {
                    reactive.insert(id.name.to_string());
                }
            }
            Statement::VariableDeclaration(vd) => collect_var_names(vd, &mut declared),
            Statement::FunctionDeclaration(f) => {
                if let Some(id) = &f.id { declared.insert(id.name.to_string()); }
            }
            Statement::ClassDeclaration(c) => {
                if let Some(id) = &c.id { declared.insert(id.name.to_string()); }
            }
            Statement::TSTypeAliasDeclaration(t) => { declared.insert(t.id.name.to_string()); }
            Statement::TSInterfaceDeclaration(i) => { declared.insert(i.id.name.to_string()); }
            Statement::TSEnumDeclaration(e) => { declared.insert(e.id.name.to_string()); }
            Statement::ImportDeclaration(imp) => {
                let Some(specs) = &imp.specifiers else { continue };
                for spec in specs {
                    let name = match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => s.local.name.as_str(),
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => s.local.name.as_str(),
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => s.local.name.as_str(),
                    };
                    declared.insert(name.to_string());
                }
            }
            Statement::ExportNamedDeclaration(exp) => {
                if let Some(decl) = &exp.declaration {
                    match decl {
                        Declaration::VariableDeclaration(vd) => collect_var_names(vd, &mut declared),
                        Declaration::FunctionDeclaration(f) => {
                            if let Some(id) = &f.id { declared.insert(id.name.to_string()); }
                        }
                        Declaration::ClassDeclaration(c) => {
                            if let Some(id) = &c.id { declared.insert(id.name.to_string()); }
                        }
                        Declaration::TSTypeAliasDeclaration(t) => { declared.insert(t.id.name.to_string()); }
                        Declaration::TSInterfaceDeclaration(i) => { declared.insert(i.id.name.to_string()); }
                        Declaration::TSEnumDeclaration(e) => { declared.insert(e.id.name.to_string()); }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (reactive, declared)
}

fn collect_var_names(vd: &VariableDeclaration<'_>, out: &mut HashSet<String>) {
    for d in &vd.declarations {
        collect_binding_pattern_names(&d.id, out);
    }
}

fn collect_binding_pattern_names(pat: &oxc::ast::ast::BindingPattern<'_>, out: &mut HashSet<String>) {
    use oxc::ast::ast::BindingPattern;
    match pat {
        BindingPattern::BindingIdentifier(id) => { out.insert(id.name.to_string()); }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties { collect_binding_pattern_names(&prop.value, out); }
            if let Some(rest) = &obj.rest { collect_binding_pattern_names(&rest.argument, out); }
        }
        BindingPattern::ArrayPattern(arr) => {
            for el in arr.elements.iter().flatten() { collect_binding_pattern_names(el, out); }
            if let Some(rest) = &arr.rest { collect_binding_pattern_names(&rest.argument, out); }
        }
        BindingPattern::AssignmentPattern(inner) => collect_binding_pattern_names(&inner.left, out),
    }
}

/// Returns the offset one-past the end of the assignment operator in `ae`.
/// Used to size the reported span like the original text scanner did.
fn span_after_left_op(ae: &oxc::ast::ast::AssignmentExpression<'_>) -> u32 {
    use oxc::syntax::operator::AssignmentOperator;
    let op_len: u32 = match ae.operator {
        AssignmentOperator::Assign => 1,
        AssignmentOperator::Addition | AssignmentOperator::Subtraction
        | AssignmentOperator::Multiplication | AssignmentOperator::Division
        | AssignmentOperator::Remainder | AssignmentOperator::BitwiseOR
        | AssignmentOperator::BitwiseAnd | AssignmentOperator::BitwiseXOR => 2,
        AssignmentOperator::ShiftLeft | AssignmentOperator::ShiftRight => 3,
        AssignmentOperator::ShiftRightZeroFill => 4,
        AssignmentOperator::Exponential => 3,
        AssignmentOperator::LogicalAnd | AssignmentOperator::LogicalOr
        | AssignmentOperator::LogicalNullish => 3,
    };
    let left_end = oxc::span::GetSpan::span(&ae.left).end;
    left_end + op_len + 1
}

/// True iff this AssignmentExpression / UpdateExpression IS the declaration
/// body of a `$: foo = expr` statement (i.e. the direct child of an
/// `ExpressionStatement` whose parent is a `$`-labeled statement). Nested
/// assignments under `$: if (...) { foo = ... }` are not skipped.
fn is_direct_reactive_decl(nodes: &oxc::semantic::AstNodes, node_id: NodeId) -> bool {
    let parent = nodes.parent_id(node_id);
    if !matches!(nodes.kind(parent), AstKind::ExpressionStatement(_)) { return false; }
    let grandparent = nodes.parent_id(parent);
    matches!(nodes.kind(grandparent), AstKind::LabeledStatement(ls) if ls.label.name == "$")
}

/// True iff `node_id`'s nearest enclosing `ExpressionStatement` is the direct
/// body of a `$`-labeled statement — i.e. the node is inside `$: <expr>;`.
/// Mirrors the old line-based `starts_with("$:")` check; the stricter ancestor
/// walk covers `$: <expr>` only, not arbitrary nesting inside `$: { ... }`.
fn is_in_direct_reactive_statement(nodes: &oxc::semantic::AstNodes, node_id: NodeId) -> bool {
    let mut id = node_id;
    loop {
        let parent = nodes.parent_id(id);
        if parent == id { return false; }
        if let AstKind::ExpressionStatement(_) = nodes.kind(parent) {
            let gp = nodes.parent_id(parent);
            return matches!(nodes.kind(gp), AstKind::LabeledStatement(ls) if ls.label.name == "$");
        }
        id = parent;
    }
}

fn expr_base_ident<'a>(expr: &'a Expression<'a>) -> Option<&'a IdentifierReference<'a>> {
    match expr {
        Expression::Identifier(id) => Some(id),
        Expression::StaticMemberExpression(m) => expr_base_ident(&m.object),
        Expression::ComputedMemberExpression(m) => expr_base_ident(&m.object),
        Expression::PrivateFieldExpression(m) => expr_base_ident(&m.object),
        _ => None,
    }
}

/// Follow a member-expression chain rooted at an identifier. Returns the base
/// `IdentifierReference` and the number of member layers above it
/// (`var` → depth 0, `var.a` → depth 1, `var.a.b` → depth 2, `var?.a` → 1).
fn expr_member_path<'a>(expr: &'a Expression<'a>) -> Option<(&'a IdentifierReference<'a>, usize)> {
    match expr {
        Expression::Identifier(id) => Some((id, 0)),
        Expression::StaticMemberExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        Expression::ComputedMemberExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        Expression::PrivateFieldExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        Expression::ChainExpression(c) => match &c.expression {
            oxc::ast::ast::ChainElement::StaticMemberExpression(m) =>
                expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
            oxc::ast::ast::ChainElement::ComputedMemberExpression(m) =>
                expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
            oxc::ast::ast::ChainElement::PrivateFieldExpression(m) =>
                expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
            _ => None,
        },
        _ => None,
    }
}

/// Member-path walk for an `AssignmentTarget` (LHS of `=`, `+=`, etc.).
fn target_member_path<'a>(target: &'a AssignmentTarget<'a>) -> Option<(&'a IdentifierReference<'a>, usize)> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some((id, 0)),
        AssignmentTarget::StaticMemberExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        AssignmentTarget::ComputedMemberExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        AssignmentTarget::PrivateFieldExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        _ => None,
    }
}

/// Peel outer `ParenthesizedExpression` wrappers (oxc preserves them when
/// parsed with `preserve_parens`, which the default parser does).
fn unwrap_paren<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut e = expr;
    while let Expression::ParenthesizedExpression(p) = e { e = &p.expression; }
    e
}

/// Walk a member-expression object through `ConditionalExpression` branches
/// (and parentheses) to find a reactive-var identifier. Returns `None` when
/// the object is a direct identifier or plain member chain — those are
/// covered by the property-mutation walk.
fn find_reactive_via_conditional<'a>(
    expr: &'a Expression<'a>,
    reactive_vars: &std::collections::HashSet<String>,
) -> Option<&'a IdentifierReference<'a>> {
    match unwrap_paren(expr) {
        Expression::ConditionalExpression(c) =>
            find_reactive_in_branch(&c.consequent, reactive_vars)
                .or_else(|| find_reactive_in_branch(&c.alternate, reactive_vars)),
        _ => None,
    }
}

fn find_reactive_in_branch<'a>(
    expr: &'a Expression<'a>,
    reactive_vars: &std::collections::HashSet<String>,
) -> Option<&'a IdentifierReference<'a>> {
    match unwrap_paren(expr) {
        Expression::Identifier(id) => {
            let name = id.name.as_str();
            if reactive_vars.contains(name)
                || (name.starts_with('$') && reactive_vars.contains(&name[1..]))
            {
                Some(id)
            } else { None }
        }
        Expression::ConditionalExpression(c) =>
            find_reactive_in_branch(&c.consequent, reactive_vars)
                .or_else(|| find_reactive_in_branch(&c.alternate, reactive_vars)),
        _ => None,
    }
}

/// Recursively collect identifier leaves from a destructure target —
/// shorthand props, renamed props, nested arrays/objects, and rest elements.
/// Member-expression variants (used for computed property assignment like
/// `x[y] = …`) are not destructure leaves and are skipped.
fn collect_target_idents<'a>(
    t: &'a AssignmentTarget<'a>,
    out: &mut Vec<&'a IdentifierReference<'a>>,
) {
    match t {
        AssignmentTarget::AssignmentTargetIdentifier(id) => out.push(id),
        AssignmentTarget::ObjectAssignmentTarget(o) => collect_obj_target_idents(o, out),
        AssignmentTarget::ArrayAssignmentTarget(a) => collect_arr_target_idents(a, out),
        _ => {}
    }
}

fn collect_obj_target_idents<'a>(
    obj: &'a ObjectAssignmentTarget<'a>,
    out: &mut Vec<&'a IdentifierReference<'a>>,
) {
    for prop in &obj.properties {
        match prop {
            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                out.push(&p.binding);
            }
            AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                collect_maybe_default_idents(&p.binding, out);
            }
        }
    }
    if let Some(rest) = &obj.rest {
        collect_target_idents(&rest.target, out);
    }
}

fn collect_arr_target_idents<'a>(
    arr: &'a ArrayAssignmentTarget<'a>,
    out: &mut Vec<&'a IdentifierReference<'a>>,
) {
    for el in arr.elements.iter().flatten() {
        collect_maybe_default_idents(el, out);
    }
    if let Some(rest) = &arr.rest {
        collect_target_idents(&rest.target, out);
    }
}

fn collect_maybe_default_idents<'a>(
    m: &'a AssignmentTargetMaybeDefault<'a>,
    out: &mut Vec<&'a IdentifierReference<'a>>,
) {
    match m {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(wd) =>
            collect_target_idents(&wd.binding, out),
        AssignmentTargetMaybeDefault::AssignmentTargetIdentifier(id) => out.push(id),
        AssignmentTargetMaybeDefault::ObjectAssignmentTarget(o) =>
            collect_obj_target_idents(o, out),
        AssignmentTargetMaybeDefault::ArrayAssignmentTarget(a) =>
            collect_arr_target_idents(a, out),
        _ => {}
    }
}

/// Member-path walk for a `SimpleAssignmentTarget` (the argument of `++`/`--`).
fn simple_target_member_path<'a>(target: &'a SimpleAssignmentTarget<'a>) -> Option<(&'a IdentifierReference<'a>, usize)> {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some((id, 0)),
        SimpleAssignmentTarget::StaticMemberExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        SimpleAssignmentTarget::ComputedMemberExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        SimpleAssignmentTarget::PrivateFieldExpression(m) =>
            expr_member_path(&m.object).map(|(i, d)| (i, d + 1)),
        _ => None,
    }
}

