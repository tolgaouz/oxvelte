//! `svelte/no-immutable-reactive-statements` — disallow reactive statements that don't reference reactive values.
//! ⭐ Recommended

use std::collections::HashMap;
use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use oxc::ast::ast::{
    AssignmentTarget, AssignmentTargetMaybeDefault, AssignmentTargetProperty, BindingPattern,
    Declaration, Expression, IdentifierReference, ImportDeclaration, ImportDeclarationSpecifier,
    ModuleExportName, SimpleAssignmentTarget, Statement, VariableDeclaration,
    VariableDeclarationKind,
};
use oxc::ast::AstKind;
use oxc::semantic::NodeId;
use std::collections::HashSet;

pub struct NoImmutableReactiveStatements;

impl Rule for NoImmutableReactiveStatements {
    fn name(&self) -> &'static str {
        "svelte/no-immutable-reactive-statements"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let script = match &ctx.ast.instance { Some(s) => s, None => return };
        let content = &script.content;
        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        let mut immutable_names: HashSet<&str> = HashSet::new();
        let mut const_names: HashSet<&str> = HashSet::new();
        let mut let_names: HashSet<&str> = HashSet::new();
        let mut prop_names: HashSet<&str> = HashSet::new();

        if let Some(module_sem) = ctx.module_semantic {
            for stmt in &module_sem.nodes().program().body {
                if let Statement::ImportDeclaration(imp) = stmt {
                    collect_import_locals(imp, &mut immutable_names);
                }
            }
        }

        if let Some(instance_sem) = ctx.instance_semantic {
            for stmt in &instance_sem.nodes().program().body {
                classify_top_level(
                    stmt,
                    &mut immutable_names,
                    &mut const_names,
                    &mut let_names,
                    &mut prop_names,
                );
            }
        }

        // Enumerate `$: ...` labeled statements at the top level directly
        // from the AST — the LabeledStatement's span covers the whole
        // `$: <body>;`, including the `$:` prefix, so slicing the script
        // content by that span gives the same `full_text` the text scanner
        // previously produced.
        let reactive_stmts: Vec<(usize, &str)> = ctx.instance_semantic
            .iter()
            .flat_map(|sem| sem.nodes().program().body.iter().filter_map(|stmt| {
                let Statement::LabeledStatement(ls) = stmt else { return None };
                if ls.label.name.as_str() != "$" { return None; }
                let start = ls.span.start as usize;
                let end = (ls.span.end as usize).min(content.len());
                Some((start, &content[start..end]))
            }))
            .collect();
        if reactive_stmts.is_empty() { return; }

        // Extract template portion of source (outside script tags) to avoid re-scanning script
        let template_source = {
            let script_start = script.span.start as usize;
            let script_end = script.span.end as usize;
            let before = &ctx.source[..script_start];
            let after = if script_end < ctx.source.len() { &ctx.source[script_end..] } else { "" };
            format!("{}{}", before, after)
        };

        // Script-side reassignments and const member writes via the AST —
        // walk each AssignmentExpression / UpdateExpression and record the
        // LHS identifier (for lets) or the base of the member chain (for
        // consts). `let x = 5` / `var x = 5` are VariableDeclarators, not
        // AssignmentExpressions, so they're not encountered here; `$: x = e`
        // assignments are skipped via the direct-reactive-body check to
        // match the old `line.starts_with("$:")` suppression.
        let mut mutable_lets: HashSet<&str> = HashSet::new();
        let mut const_member_written: HashSet<&str> = HashSet::new();
        if let Some(sem) = ctx.instance_semantic {
            let nodes = sem.nodes();
            for node in nodes.iter() {
                if is_in_direct_reactive_body(nodes, node.id()) { continue; }
                match node.kind() {
                    AstKind::AssignmentExpression(ae) => {
                        record_assign_target(&ae.left, &let_names, &const_names,
                            &mut mutable_lets, &mut const_member_written);
                    }
                    AstKind::UpdateExpression(ue) => {
                        record_simple_target(&ue.argument, &let_names, &const_names,
                            &mut mutable_lets, &mut const_member_written);
                    }
                    _ => {}
                }
            }
        }

        // Template-side writes — keeps string scanning for now; template
        // expressions are stored as opaque text in AttributeValue and a full
        // AST walk here would require per-expression re-parsing.
        for &var in &let_names {
            if !mutable_lets.contains(var) && has_reassignment(&template_source, var) {
                mutable_lets.insert(var);
            }
        }
        for &var in &const_names {
            if !const_member_written.contains(var) && has_member_write(&template_source, var) {
                const_member_written.insert(var);
            }
        }

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::Binding, span, .. } = attr {
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(open) = region.find('{') {
                            if let Some(close) = region.find('}') {
                                let expr = region[open+1..close].trim();
                                if let_names.contains(expr) {
                                    mutable_lets.insert(expr);
                                }
                                let base = expr.split('.').next().unwrap_or(expr);
                                if base != expr && let_names.contains(base) {
                                    mutable_lets.insert(base);
                                }
                                for &var in &const_names {
                                    if expr.starts_with(var) && expr.len() > var.len() {
                                        let next = expr.as_bytes()[var.len()];
                                        if next == b'.' || next == b'[' {
                                            const_member_written.insert(var);
                                        }
                                    }
                                }
                            }
                        }
                        if !region.contains('{') && !region.contains('=') {
                            if let Some(colon) = region.find(':') {
                                let name = region[colon+1..].trim();
                                if let_names.contains(name) {
                                    mutable_lets.insert(name);
                                }
                            }
                        }
                    }
                }
            }
        });

        for &var in &const_names {
            if !const_member_written.contains(var) {
                immutable_names.insert(var);
            }
        }

        let (each_iterable_names, const_tag_names) = collect_each_and_const_names(&ctx.ast.html);
        immutable_names.retain(|n| !each_iterable_names.contains(*n) || const_tag_names.contains(*n));

        let all_immutable: HashSet<&str> = immutable_names.iter().copied()
            .chain(let_names.iter()
                .filter(|n| !mutable_lets.contains(*n) && !prop_names.contains(*n))
                .copied())
            .collect();

        let ast_immutable_stmts = check_immutability_ast(ctx.instance_semantic, &all_immutable);

        let mut reactive_decl_names: HashSet<&str> = HashSet::new();
        for (_, full_text) in &reactive_stmts {
            if full_text.len() < 3 { continue; }
            let after = full_text[2..].trim_start();
            if let Some(eq) = after.find('=') {
                let name = after[..eq].trim();
                if name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !name.is_empty() {
                    reactive_decl_names.insert(name);
                }
            }
        }

        let all_declared: HashSet<&str> = all_immutable.iter().copied()
            .chain(mutable_lets.iter().copied())
            .chain(prop_names.iter().copied())
            .collect();

        for &(offset, ref full_text) in &reactive_stmts {
            if full_text.len() < 3 { continue; }
            let after = full_text[2..].trim_start();
            let rhs = if let Some(eq) = after.find('=') {
                let lhs = after[..eq].trim();
                let post = &after[eq + 1..];
                if post.starts_with('=') || post.starts_with('>') {
                    after
                } else if lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !lhs.is_empty() {
                    post
                } else {
                    after
                }
            } else {
                after
            };

            let mut ids = extract_identifiers(rhs);

            if rhs == after {
                let write_targets = extract_assignment_targets(rhs);
                if !write_targets.is_empty() {
                    let mut write_counts: HashMap<&str, usize> = HashMap::new();
                    for t in &write_targets {
                        *write_counts.entry(t).or_insert(0) += 1;
                    }
                    let mut id_counts: HashMap<String, usize> = HashMap::new();
                    for id in &ids {
                        *id_counts.entry(id.clone()).or_insert(0) += 1;
                    }
                    ids.retain(|id| {
                        id_counts.get(id).copied().unwrap_or(0) > write_counts.get(id.as_str()).copied().unwrap_or(0)
                    });
                }
            }

            if ids.iter().any(|id| id.starts_with('$')) { continue; }
            if ids.iter().any(|id| reactive_decl_names.contains(id.as_str())) { continue; }

            let referenced: Vec<&str> = ids.iter()
                .filter(|id| all_declared.contains(id.as_str()))
                .map(|s| s.as_str())
                .collect();

            let local_names = collect_local_names(rhs);
            let has_unknown = ids.iter().any(|id| {
                !all_declared.contains(id.as_str()) && !local_names.contains(id.as_str())
            });

            let text_flag = !has_unknown && ((!referenced.is_empty() && referenced.iter().all(|v| all_immutable.contains(v)))
                || (ids.is_empty() && rhs != after));
            let ast_flag = (has_unknown || !text_flag) && {
                let line_num = content[..offset].matches('\n').count();
                ast_immutable_stmts.iter().any(|&s| content[..s as usize].matches('\n').count() == line_num)
            };
            if (text_flag || ast_flag) && full_text.len() >= 3 {
                let after = full_text[2..].trim_start();
                let body_off = full_text.len() - after.len();
                let base = content_offset + offset;
                let diag_start = after.find('=').and_then(|eq| {
                    let (lhs, post) = (after[..eq].trim(), &after[eq + 1..]);
                    (!post.starts_with('=') && !post.starts_with('>') && !lhs.is_empty()
                        && lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'))
                        .then(|| base + full_text.len() - full_text[body_off + eq + 1..].trim_start().len())
                }).unwrap_or(base + body_off);
                ctx.diagnostic(
                    "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                    oxc::span::Span::new(diag_start as u32, (base + full_text.len()) as u32),
                );
            }
        }
    }
}

/// Classify a top-level statement's declarations into `immutable`, `const`,
/// `let`, or `prop` (Svelte `export let x`) buckets. Non-declarations are
/// ignored.
fn classify_top_level<'a>(
    stmt: &Statement<'a>,
    immutable: &mut HashSet<&'a str>,
    const_names: &mut HashSet<&'a str>,
    let_names: &mut HashSet<&'a str>,
    prop_names: &mut HashSet<&'a str>,
) {
    match stmt {
        Statement::ImportDeclaration(imp) => collect_import_locals(imp, immutable),
        Statement::FunctionDeclaration(f) => {
            if let Some(id) = &f.id { immutable.insert(id.name.as_str()); }
        }
        Statement::ClassDeclaration(c) => {
            if let Some(id) = &c.id { immutable.insert(id.name.as_str()); }
        }
        Statement::TSTypeAliasDeclaration(t) => { immutable.insert(t.id.name.as_str()); }
        Statement::TSInterfaceDeclaration(i) => { immutable.insert(i.id.name.as_str()); }
        Statement::TSEnumDeclaration(e) => { immutable.insert(e.id.name.as_str()); }
        Statement::VariableDeclaration(vd) => {
            collect_var_decl(vd, const_names, let_names);
        }
        Statement::ExportNamedDeclaration(exp) => {
            if let Some(decl) = &exp.declaration {
                match decl {
                    Declaration::VariableDeclaration(vd) => match vd.kind {
                        VariableDeclarationKind::Let | VariableDeclarationKind::Var => {
                            for d in &vd.declarations {
                                collect_binding_names(&d.id, prop_names);
                            }
                        }
                        VariableDeclarationKind::Const | VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => {
                            for d in &vd.declarations {
                                collect_binding_names(&d.id, const_names);
                            }
                        }
                    },
                    Declaration::FunctionDeclaration(f) => {
                        if let Some(id) = &f.id { immutable.insert(id.name.as_str()); }
                    }
                    Declaration::ClassDeclaration(c) => {
                        if let Some(id) = &c.id { immutable.insert(id.name.as_str()); }
                    }
                    Declaration::TSTypeAliasDeclaration(t) => { immutable.insert(t.id.name.as_str()); }
                    Declaration::TSInterfaceDeclaration(i) => { immutable.insert(i.id.name.as_str()); }
                    Declaration::TSEnumDeclaration(e) => { immutable.insert(e.id.name.as_str()); }
                    _ => {}
                }
            }
            // `export { x }` (no `from`) marks locals as Svelte props.
            if exp.source.is_none() {
                for spec in &exp.specifiers {
                    let name = match &spec.local {
                        ModuleExportName::IdentifierName(n) => n.name.as_str(),
                        ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                        ModuleExportName::StringLiteral(l) => l.value.as_str(),
                    };
                    prop_names.insert(name);
                }
            }
        }
        _ => {}
    }
}

fn collect_import_locals<'a>(imp: &ImportDeclaration<'a>, out: &mut HashSet<&'a str>) {
    let Some(specifiers) = &imp.specifiers else { return };
    for spec in specifiers {
        match spec {
            ImportDeclarationSpecifier::ImportSpecifier(s) => { out.insert(s.local.name.as_str()); }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => { out.insert(s.local.name.as_str()); }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => { out.insert(s.local.name.as_str()); }
        }
    }
}

fn collect_var_decl<'a>(
    vd: &VariableDeclaration<'a>,
    const_names: &mut HashSet<&'a str>,
    let_names: &mut HashSet<&'a str>,
) {
    let target = match vd.kind {
        VariableDeclarationKind::Const | VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => const_names,
        VariableDeclarationKind::Let | VariableDeclarationKind::Var => let_names,
    };
    for d in &vd.declarations {
        collect_binding_names(&d.id, target);
    }
}

fn collect_binding_names<'a>(pat: &BindingPattern<'a>, out: &mut HashSet<&'a str>) {
    match pat {
        BindingPattern::BindingIdentifier(id) => { out.insert(id.name.as_str()); }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_names(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_binding_names(el, out);
            }
            if let Some(rest) = &arr.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(inner) => {
            collect_binding_names(&inner.left, out);
        }
    }
}

/// Walk down an expression chain rooted at an identifier — used to locate
/// the base identifier of a member-expression LHS.
fn expr_base_ident<'a>(expr: &'a Expression<'a>) -> Option<&'a IdentifierReference<'a>> {
    match expr {
        Expression::Identifier(id) => Some(id),
        Expression::StaticMemberExpression(m) => expr_base_ident(&m.object),
        Expression::ComputedMemberExpression(m) => expr_base_ident(&m.object),
        Expression::PrivateFieldExpression(m) => expr_base_ident(&m.object),
        _ => None,
    }
}

/// Recursively collect leaf identifiers from a destructure pattern —
/// `{a, b: c, ...d}` / `[a, b, ...c]` / nested patterns / defaults.
fn collect_destructure_idents<'a>(
    target: &'a AssignmentTarget<'a>,
    out: &mut Vec<&'a IdentifierReference<'a>>,
) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => out.push(id),
        AssignmentTarget::ObjectAssignmentTarget(obj) => {
            for prop in &obj.properties {
                match prop {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) =>
                        out.push(&p.binding),
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) =>
                        collect_destructure_maybe_default(&p.binding, out),
                }
            }
            if let Some(rest) = &obj.rest { collect_destructure_idents(&rest.target, out); }
        }
        AssignmentTarget::ArrayAssignmentTarget(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_destructure_maybe_default(el, out);
            }
            if let Some(rest) = &arr.rest { collect_destructure_idents(&rest.target, out); }
        }
        _ => {}
    }
}

fn collect_destructure_maybe_default<'a>(
    m: &'a AssignmentTargetMaybeDefault<'a>,
    out: &mut Vec<&'a IdentifierReference<'a>>,
) {
    match m {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(wd) =>
            collect_destructure_idents(&wd.binding, out),
        AssignmentTargetMaybeDefault::AssignmentTargetIdentifier(id) => out.push(id),
        AssignmentTargetMaybeDefault::ObjectAssignmentTarget(obj) => {
            for prop in &obj.properties {
                match prop {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) =>
                        out.push(&p.binding),
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) =>
                        collect_destructure_maybe_default(&p.binding, out),
                }
            }
            if let Some(rest) = &obj.rest { collect_destructure_idents(&rest.target, out); }
        }
        AssignmentTargetMaybeDefault::ArrayAssignmentTarget(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_destructure_maybe_default(el, out);
            }
            if let Some(rest) = &arr.rest { collect_destructure_idents(&rest.target, out); }
        }
        _ => {}
    }
}

/// Record writes reachable through an AssignmentExpression's LHS:
///   - `let_name = ...` / destructure into a let → mutable let
///   - `const_name.prop = ...` / `const_name[x] = ...` → const member write
fn record_assign_target<'a>(
    target: &'a AssignmentTarget<'a>,
    let_names: &HashSet<&'a str>,
    const_names: &HashSet<&'a str>,
    mutable_lets: &mut HashSet<&'a str>,
    const_member_written: &mut HashSet<&'a str>,
) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            let name = id.name.as_str();
            if let_names.contains(name) { mutable_lets.insert(name); }
        }
        AssignmentTarget::StaticMemberExpression(m) => {
            if let Some(id) = expr_base_ident(&m.object) {
                let name = id.name.as_str();
                if const_names.contains(name) { const_member_written.insert(name); }
            }
        }
        AssignmentTarget::ComputedMemberExpression(m) => {
            if let Some(id) = expr_base_ident(&m.object) {
                let name = id.name.as_str();
                if const_names.contains(name) { const_member_written.insert(name); }
            }
        }
        AssignmentTarget::PrivateFieldExpression(m) => {
            if let Some(id) = expr_base_ident(&m.object) {
                let name = id.name.as_str();
                if const_names.contains(name) { const_member_written.insert(name); }
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(_)
        | AssignmentTarget::ArrayAssignmentTarget(_) => {
            let mut idents = Vec::new();
            collect_destructure_idents(target, &mut idents);
            for id in idents {
                let name = id.name.as_str();
                if let_names.contains(name) { mutable_lets.insert(name); }
            }
        }
        _ => {}
    }
}

/// Same as `record_assign_target` but for an UpdateExpression's argument
/// (`++`/`--`). Destructure variants are syntactically impossible here, so
/// only identifier and member variants need to be considered.
fn record_simple_target<'a>(
    target: &'a SimpleAssignmentTarget<'a>,
    let_names: &HashSet<&'a str>,
    const_names: &HashSet<&'a str>,
    mutable_lets: &mut HashSet<&'a str>,
    const_member_written: &mut HashSet<&'a str>,
) {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
            let name = id.name.as_str();
            if let_names.contains(name) { mutable_lets.insert(name); }
        }
        SimpleAssignmentTarget::StaticMemberExpression(m) => {
            if let Some(id) = expr_base_ident(&m.object) {
                let name = id.name.as_str();
                if const_names.contains(name) { const_member_written.insert(name); }
            }
        }
        SimpleAssignmentTarget::ComputedMemberExpression(m) => {
            if let Some(id) = expr_base_ident(&m.object) {
                let name = id.name.as_str();
                if const_names.contains(name) { const_member_written.insert(name); }
            }
        }
        SimpleAssignmentTarget::PrivateFieldExpression(m) => {
            if let Some(id) = expr_base_ident(&m.object) {
                let name = id.name.as_str();
                if const_names.contains(name) { const_member_written.insert(name); }
            }
        }
        _ => {}
    }
}

/// True iff the node's nearest enclosing ExpressionStatement is the direct
/// body of a `$`-labeled statement (`$: <expr>;`). Matches the old
/// `line.starts_with("$:")` suppression so reactive declarations aren't
/// counted as reassignments.
fn is_in_direct_reactive_body(nodes: &oxc::semantic::AstNodes, node_id: NodeId) -> bool {
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

fn has_reassignment(content: &str, var: &str) -> bool {
    let suffixes: &[&str] = &[" =", "=", "++", "--", " +=", " -="];
    // Single scan for the var, check suffixes at each match
    for (pos, _) in content.match_indices(var) {
        if pos > 0 && matches!(content.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b':') { continue; }
        // Prefix `++var` / `--var` is a write that suffix-only matching misses.
        if pos >= 2 {
            let pref = &content.as_bytes()[pos - 2..pos];
            if pref == b"++" || pref == b"--" {
                let ls = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line = content[ls..].trim_start();
                if !line.starts_with("let ") && !line.starts_with("var ") && !line.starts_with("$:") {
                    return true;
                }
            }
        }
        let after = &content[pos + var.len()..];
        let Some(suffix) = suffixes.iter().find(|s| after.starts_with(*s)) else { continue };
        let search_len = var.len() + suffix.len();
        let ls = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line = content[ls..].trim_start();
        if line.starts_with("let ") || line.starts_with("var ") || line.starts_with("$:") { continue; }
        if matches!(*suffix, " =" | "=") && pos + search_len < content.len() && content.as_bytes()[pos + search_len] == b'=' { continue; }
        return true;
    }

    for pat in &[format!("[{}]", var), format!(", {}]", var), format!("[{},", var),
                  format!(": {} }}", var), format!(": {}}}", var)] {
        if content.match_indices(pat.as_str()).any(|(pos, _)| {
            let rest = content[pos + pat.len()..].trim_start();
            rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>") && {
                let ls = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line = content[ls..].trim_start();
                !line.starts_with("let ") && !line.starts_with("var ") && !line.starts_with("const ") && !line.starts_with("$:")
            }
        }) { return true; }
    }
    false
}

fn has_member_write(content: &str, var: &str) -> bool {
    let skip_brackets = |bytes: &[u8], i: &mut usize| {
        let mut d = 1; *i += 1;
        while *i < bytes.len() && d > 0 {
            match bytes[*i] { b'[' => d += 1, b']' => d -= 1, _ => {} }
            *i += 1;
        }
    };
    // Single scan for var, check if followed by . or [
    for (pos, _) in content.match_indices(var) {
        if pos > 0 && matches!(content.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') { continue; }
        let after_var = pos + var.len();
        if after_var >= content.len() { continue; }
        let first = content.as_bytes()[after_var];
        if first != b'.' && first != b'[' { continue; }
        let rest = &content[after_var..];
        let mut i = 0;
        let bytes = rest.as_bytes();
        if bytes[0] == b'[' {
            skip_brackets(bytes, &mut i);
        } else {
            i += 1; // skip '.'
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
        }
        while i < bytes.len() {
            match bytes[i] {
                b'.' => { i += 1; while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; } }
                b'[' => skip_brackets(bytes, &mut i),
                _ => break,
            }
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() { i += 1; }
        if i < bytes.len() {
            match bytes[i] {
                b'=' if i + 1 < bytes.len() && bytes[i + 1] != b'=' => return true,
                b'+' | b'-' if i + 1 < bytes.len() && bytes[i + 1] == bytes[i] => return true,
                b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'
                    if i + 1 < bytes.len() && bytes[i + 1] == b'=' => return true,
                _ => {}
            }
        }
    }
    false
}

fn extract_assignment_targets(expr: &str) -> HashSet<&str> {
    let mut targets = HashSet::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => { i = skip_simple_string(bytes, i); continue; }
            b'`' => { let (end, _) = skip_template_literal(bytes, i); i = end; continue; }
            b'{' | b'(' | b'[' => { depth += 1; i += 1; }
            b'}' | b')' | b']' => { depth -= 1; i += 1; }
            b'=' if i + 1 < bytes.len() && bytes[i + 1] != b'=' && bytes[i + 1] != b'>' => {
                let mut j = i;
                while j > 0 && bytes[j - 1].is_ascii_whitespace() { j -= 1; }
                let end = j;
                while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_' || bytes[j - 1] == b'$') { j -= 1; }
                if j < end && (j == 0 || (bytes[j - 1] != b'.' && !bytes[j - 1].is_ascii_alphanumeric() && bytes[j - 1] != b'_')) {
                    targets.insert(&expr[j..end]);
                }
                i += 1;
            }
            _ => { i += 1; }
        }
    }
    targets
}

fn collect_local_names(expr: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => { i = skip_simple_string(bytes, i); continue; }
            b'`' => { let (end, _) = skip_template_literal(bytes, i); i = end; continue; }
            b'=' if i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                let mut j = i;
                while j > 0 && bytes[j - 1].is_ascii_whitespace() { j -= 1; }
                if j >= 2 && bytes[j - 1] == b')' {
                    let mut depth = 1;
                    let mut k = j - 2;
                    while depth > 0 {
                        match bytes[k] { b')' => depth += 1, b'(' => depth -= 1, _ => {} }
                        if depth > 0 { if k == 0 { break; } k -= 1; }
                    }
                    extract_param_names(&expr[k + 1..j - 1], &mut names);
                } else {
                    let end = j;
                    while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_' || bytes[j - 1] == b'$') { j -= 1; }
                    if j < end { names.insert(expr[j..end].to_string()); }
                }
                i += 2; continue;
            }
            _ => {}
        }
        if i + 8 < bytes.len() && expr.is_char_boundary(i) && expr.is_char_boundary(i + 8) && &expr[i..i + 8] == "function" {
            let rest = expr[i + 8..].trim_start();
            let offset = expr.len() - rest.len();
            if let Some(open) = rest.find('(') {
                if let Some(close) = rest[open..].find(')') {
                    extract_param_names(&rest[open + 1..open + close], &mut names);
                    i = offset + open + close + 1; continue;
                }
            }
        }
        for kw in &["const ", "let ", "var "] {
            if i + kw.len() <= bytes.len() && expr.is_char_boundary(i) && expr.is_char_boundary(i + kw.len())
                && &expr[i..i + kw.len()] == *kw && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
                let rest = expr[i + kw.len()..].trim_start();
                let rest_offset = expr.len() - rest.len();
                let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').unwrap_or(rest.len());
                if end > 0 { names.insert(rest[..end].to_string()); }
                i = rest_offset + end; break;
            }
        }
        i += 1;
    }
    names
}

fn extract_param_names(params: &str, names: &mut HashSet<String>) {
    for p in params.split(',').map(str::trim) {
        if matches!(p.as_bytes().first(), Some(b'{' | b'[' | b'.')) { continue; }
        let name = p.split(|c: char| c == ':' || c == '=').next().unwrap_or("").trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            names.insert(name.to_string());
        }
    }
}

fn skip_template_literal(bytes: &[u8], mut i: usize) -> (usize, Vec<(usize, usize)>) {
    let mut interpolations = Vec::new();
    i += 1;
    while i < bytes.len() && bytes[i] != b'`' {
        if bytes[i] == b'\\' { i += 2; continue; }
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            i += 2;
            let start = i;
            let mut d = 1;
            while i < bytes.len() && d > 0 {
                match bytes[i] { b'{' => d += 1, b'}' => d -= 1, _ => {} }
                if d > 0 { i += 1; }
            }
            interpolations.push((start, i));
            if i < bytes.len() { i += 1; }
            continue;
        }
        i += 1;
    }
    if i < bytes.len() { i += 1; }
    (i, interpolations)
}

fn is_js_keyword_or_builtin(id: &str) -> bool {
    matches!(id, "true" | "false" | "null" | "undefined" | "new" | "typeof"
        | "if" | "else" | "return" | "const" | "let" | "var" | "function"
        | "class" | "this" | "console" | "Math" | "JSON" | "Object" | "Array"
        | "String" | "Number" | "Boolean" | "Date" | "Error" | "Promise"
        | "Map" | "Set" | "RegExp" | "Symbol" | "BigInt" | "Infinity" | "NaN"
        | "void" | "delete" | "instanceof" | "in" | "of" | "switch" | "case"
        | "break" | "continue" | "throw" | "try" | "catch" | "finally"
        | "for" | "while" | "do" | "async" | "await" | "yield"
        | "satisfies" | "as" | "super" | "with" | "debugger"
        | "default" | "export" | "from")
}

fn skip_simple_string(bytes: &[u8], i: usize) -> usize {
    let q = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() && bytes[j] != q {
        if bytes[j] == b'\\' { j += 1; }
        j += 1;
    }
    if j < bytes.len() { j + 1 } else { j }
}

fn extract_identifiers(expr: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
                if i + 1 < bytes.len() { i += 2; }
            }
            b'\'' | b'"' => { i = skip_simple_string(bytes, i); }
            b'`' => {
                let (end, interps) = skip_template_literal(bytes, i);
                for (s, e) in interps { ids.extend(extract_identifiers(&expr[s..e])); }
                i = end;
            }
            b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
                if i > 0 && bytes[i - 1] == b'.' && !(i >= 3 && bytes[i - 2] == b'.' && bytes[i - 3] == b'.') {
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                    continue;
                }
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                let id = &expr[start..i];
                if !is_js_keyword_or_builtin(id) {
                    let next = expr[i..].trim_start();
                    let is_obj_key = next.starts_with(':') && !next.starts_with("::") && {
                        let b = expr[..start].trim_end();
                        b.ends_with('{') || b.ends_with(',') || b.ends_with('\n')
                    };
                    if !is_obj_key { ids.push(id.to_string()); }
                }
            }
            _ => { i += 1; }
        }
    }
    ids
}

fn check_immutability_ast(
    semantic: Option<&oxc::semantic::Semantic<'_>>,
    text_immutable: &HashSet<&str>,
) -> HashSet<u32> {
    if text_immutable.is_empty() { return HashSet::new(); }
    let Some(semantic) = semantic else { return HashSet::new() };
    use oxc::ast::AstKind;
    use oxc::span::GetSpan;

    let mut result = HashSet::new();
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let root_scope = scoping.root_scope_id();

    // Find all LabeledStatement with label "$"
    for node in nodes.iter() {
        let AstKind::LabeledStatement(labeled) = node.kind() else { continue };
        if labeled.label.name.as_str() != "$" { continue; }

        let stmt_start = labeled.span.start;
        let stmt_end = labeled.span.end;

        // Determine if this is a simple assignment: `$: var = expr`
        let is_simple_assign = matches!(
            &labeled.body,
            oxc::ast::ast::Statement::ExpressionStatement(es)
            if matches!(&es.expression, oxc::ast::ast::Expression::AssignmentExpression(_))
        );

        // Collect all value-level identifier references within this statement
        let mut has_any_ref = false;
        let mut all_refs_immutable = true;
        let mut has_store_ref = false;

        for desc in nodes.iter() {
            let AstKind::IdentifierReference(ident) = desc.kind() else { continue };
            let sp = ident.span;
            if sp.start < stmt_start || sp.end > stmt_end { continue; }

            let name = ident.name.as_str();

            if name.starts_with('$') { has_store_ref = true; continue; }

            let parent_id = nodes.parent_id(desc.id());
            if matches!(nodes.kind(parent_id), AstKind::StaticMemberExpression(m) if m.property.span == ident.span) { continue; }
            if is_simple_assign && matches!(nodes.kind(parent_id), AstKind::AssignmentExpression(a) if a.left.span().start == ident.span.start) { continue; }
            if ident.reference_id.get().is_some_and(|r| { let rf = scoping.get_reference(r); rf.is_write() && !rf.is_read() }) { continue; }

            let symbol = ident.reference_id.get().and_then(|r| scoping.get_reference(r).symbol_id());
            match symbol {
                Some(sym) => {
                    has_any_ref = true;
                    if scoping.symbol_scope_id(sym) == root_scope
                        && !text_immutable.contains(scoping.symbol_name(sym))
                    {
                        all_refs_immutable = false;
                    }
                }
                None => {
                    if !is_known_js_global(name) { all_refs_immutable = false; }
                }
            }
        }

        if has_store_ref { continue; }
        if has_any_ref && all_refs_immutable {
            result.insert(stmt_start);
        }
    }

    result
}

fn collect_each_and_const_names(fragment: &crate::ast::Fragment) -> (HashSet<String>, HashSet<String>) {
    let (mut each_names, mut const_names) = (HashSet::new(), HashSet::new());
    let is_ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$');
    walk_template_nodes(fragment, &mut |node| match node {
        TemplateNode::EachBlock(each) => {
            let e = each.expression.trim();
            if is_ident(e) { each_names.insert(e.to_string()); }
        }
        TemplateNode::ConstTag(ct) => {
            if let Some(eq) = ct.declaration.find('=') {
                let lhs = ct.declaration[..eq].trim();
                if is_ident(lhs) { const_names.insert(lhs.to_string()); }
            }
        }
        _ => {}
    });
    (each_names, const_names)
}

fn is_known_js_global(name: &str) -> bool {
    matches!(name,
        "Object" | "Array" | "String" | "Number" | "Boolean" | "Date" | "Error"
        | "Promise" | "Map" | "Set" | "WeakMap" | "WeakSet" | "RegExp" | "Symbol"
        | "BigInt" | "Math" | "JSON" | "Infinity" | "NaN" | "undefined"
        | "parseInt" | "parseFloat" | "isNaN" | "isFinite"
        | "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent"
        | "console" | "globalThis"
        | "Proxy" | "Reflect" | "WeakRef"
        | "ArrayBuffer" | "SharedArrayBuffer" | "DataView"
        | "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array"
        | "Uint32Array" | "Int32Array" | "Float32Array" | "Float64Array"
        | "Intl" | "Atomics"
    )
}
