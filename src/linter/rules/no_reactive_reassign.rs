//! `svelte/no-reactive-reassign` — disallow reassignment of reactive values.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use oxc::ast::ast::{
    AssignmentTarget, Declaration, Expression, ImportDeclarationSpecifier, SimpleAssignmentTarget,
    Statement, VariableDeclaration,
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
            for var in &reactive_vars {
                if !content.contains(var.as_str()) { continue; }
                // Single "var." scan for mutating methods, chained mutations, and ++/--
                {
                    let prefix = format!("{}.", var);
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(prefix.as_str()) {
                        let abs = search_from + pos;
                        search_from = abs + prefix.len();
                        if abs > 0 && matches!(content.as_bytes()[abs - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_') { continue; }
                        let after_prefix = &content[abs + prefix.len()..];

                        // Check direct mutating method: var.push(
                        if let Some(method) = MUTATING_METHODS.iter().find(|m| after_prefix.starts_with(*m)) {
                            let ls = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                            if !content[ls..].trim_start().starts_with("$:") && !is_shadowed_in_function(semantic, abs, var) {
                                let sp = content_offset + abs;
                                ctx.diagnostic(format!("Assignment to reactive value '{}'.", var),
                                    oxc::span::Span::new(sp as u32, (sp + prefix.len() + method.len()) as u32));
                            }
                            continue;
                        }

                        // Follow property chain for chained mutations, assignments, and ++/--
                        let mut rest = after_prefix;
                        let mut chain_len = prefix.len();
                        loop {
                            let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
                            if end == 0 { break; }
                            rest = &rest[end..];
                            chain_len += end;

                            // Check for ++ and -- on property
                            if rest.starts_with("++") || rest.starts_with("--") {
                                let ls = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                                if !content[ls..].trim_start().starts_with("$:") && !is_shadowed_in_function(semantic, abs, var) {
                                    let sp = content_offset + abs;
                                    ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                        oxc::span::Span::new(sp as u32, (sp + chain_len + 2) as u32));
                                }
                                break;
                            }

                            if rest.starts_with('.') || rest.starts_with("?.") {
                                let skip = if rest.starts_with("?.") { 2 } else { 1 };
                                rest = &rest[skip..];
                                chain_len += skip;
                                // Check for chained mutating method
                                for m in MUTATING_METHODS {
                                    if rest.starts_with(*m) {
                                        let ls = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                                        if !content[ls..].trim_start().starts_with("$:") && !is_shadowed_in_function(semantic, abs, var) {
                                            let sp = content_offset + abs;
                                            ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                                oxc::span::Span::new(sp as u32, (sp + chain_len + m.len() - 1) as u32));
                                        }
                                    }
                                }
                            } else if let Some(close) = rest.strip_prefix('[').and_then(|r| r.find(']')) {
                                rest = &rest[close + 2..];
                                chain_len += close + 2;
                            } else {
                                break;
                            }
                        }
                    }
                }
                for pattern_base in &[format!("{}.", var), format!("{}?.", var), format!("{}[", var)] {
                    for (pos, _) in content.match_indices(pattern_base.as_str()) {
                        if pos > 0 && matches!(content.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_') { continue; }
                        let ls = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        if content[ls..].trim_start().starts_with("$:") || is_shadowed_in_function(semantic, pos, var) { continue; }
                        let after = &content[pos + pattern_base.len()..];
                        let mut rest = if pattern_base.ends_with('[') {
                            after.find(']').map(|p| &after[p+1..]).unwrap_or("")
                        } else {
                            let end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
                            &after[end..]
                        };
                        loop {
                            if rest.starts_with('.') || rest.starts_with("?.") {
                                let skip = if rest.starts_with("?.") { 2 } else { 1 };
                                let r = &rest[skip..];
                                let end = r.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(r.len());
                                rest = &r[end..];
                            } else if rest.starts_with('[') {
                                rest = rest[1..].find(']').map(|p| &rest[p+2..]).unwrap_or("");
                            } else {
                                break;
                            }
                        }
                        let rest = rest.trim_start();
                        if rest.starts_with('=') && !rest.starts_with("==") {
                            let sp = content_offset + pos;
                            ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                oxc::span::Span::new(sp as u32, (sp + pattern_base.len()) as u32));
                        }
                    }
                }
                let delete_pattern = format!("delete {}", var);
                for (pos, _) in content.match_indices(&delete_pattern) {
                    let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let line = content[line_start..].trim_start();
                    if line.starts_with("$:") { continue; }
                    let sp = content_offset + pos;
                    ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                        oxc::span::Span::new(sp as u32, (sp + delete_pattern.len()) as u32));
                }
            }
        } // end if check_props

        for var in &reactive_vars {
            if !content.contains(var.as_str()) { continue; }
            let destructure_patterns = [
                format!("{} }} =", var),     // { foo: reactiveVar } =
                format!("{}}} =", var),      // {reactiveVar} = (no space)
                format!("{}] =", var),       // [reactiveVar] =
                format!("{}]] =", var),      // [[reactiveVar]] = (nested)
                format!("...{} }} =", var),  // { ...reactiveVar } =
                format!("...{}] =", var),    // [...reactiveVar] =
            ];
            for pattern in &destructure_patterns {
                if let Some(pos) = content.find(pattern.as_str()) {
                    let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let line = content[line_start..].trim_start();
                    if line.starts_with("$:") || line.starts_with("const ")
                        || line.starts_with("let ") || line.starts_with("var ") { continue; }

                    if pattern.ends_with("] =") && !pattern.ends_with("]] =") && !pattern.starts_with("...") {
                        let before = &content[..pos];
                        if let Some(bracket_pos) = before.rfind('[') {
                            let between = content[bracket_pos + 1..pos].trim();
                            if between.is_empty() {
                                let before_bracket = content[..bracket_pos].trim_end();
                                if !(before_bracket.ends_with('=')
                                    || before_bracket.ends_with(',')
                                    || before_bracket.ends_with(';')
                                    || before_bracket.ends_with('{')
                                    || before_bracket.ends_with('(')
                                    || before_bracket.is_empty()
                                    || before_bracket.ends_with('\n'))
                                {
                                    continue;
                                }
                            } else {
                                if !between.contains(',') {
                                    continue; // computed property access
                                }
                            }
                        }
                    }

                    let sp = content_offset + pos;
                    ctx.diagnostic(format!("Assignment to reactive value '{}'.", var),
                        oxc::span::Span::new(sp as u32, (sp + var.len()) as u32));
                    break; // Only report once per var per pattern type
                }
            }
        }

        let has_for = content.contains("for (");
        for var in &reactive_vars {
            if !has_for { break; }
            if !content.contains(var.as_str()) { continue; }
            let for_patterns = [
                format!("for ({} ", var),
                format!("for ({}", var),
                format!("for (const {} ", var),
                format!("for (let {} ", var),
            ];
            let member_for = if check_props {
                vec![
                    format!("for ({}.", var),
                    format!("for (const {}.", var),
                    format!("for (let {}.", var),
                ]
            } else {
                vec![]
            };
            for pattern in member_for.iter().chain(for_patterns.iter()) {
                if let Some(pos) = content.find(pattern.as_str()) {
                    let after = &content[pos + pattern.len()..];
                    if after.contains(" of ") || after.contains(" in ") {
                        let sp = content_offset + pos;
                        ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                            oxc::span::Span::new(sp as u32, (sp + pattern.len()) as u32));
                    }
                }
            }
        }

        if check_props { for var in &reactive_vars {
            if !content.contains(var.as_str()) { continue; }
            let ternary_pat1 = format!("? {} :", var);
            let ternary_pat2 = format!("? {}", var);
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("$:") { continue; }
                if !trimmed.contains(var.as_str()) { continue; }
                if trimmed.contains(&ternary_pat1)
                    || trimmed.contains(&ternary_pat2)
                {
                    if let Some(dot_pos) = trimmed.rfind(").") {
                        let after_dot = &trimmed[dot_pos + 2..];
                        let end = after_dot.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after_dot.len());
                        let rest = after_dot[end..].trim_start();
                        if rest.starts_with('=') && !rest.starts_with("==") {
                            if let Some(pos) = content.find(trimmed) {
                                let sp = content_offset + pos;
                                ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                    oxc::span::Span::new(sp as u32, (sp + trimmed.len()) as u32));
                            }
                        }
                    }
                }
            }
        }} // end if check_props (conditional member assignment)

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

/// Returns true iff `pos` is inside a function body whose scope declares
/// `var_name`. Uses semantic's scoping — walks the node at `pos` up to its
/// enclosing function scope and checks whether `var_name` is a local binding
/// anywhere on the way. We intentionally stop at the first function boundary,
/// matching the old rule's "nearest enclosing function" behavior.
fn is_shadowed_in_function(semantic: &Semantic<'_>, pos: usize, var_name: &str) -> bool {
    let target = pos as u32;
    let nodes = semantic.nodes();
    let scoping = semantic.scoping();
    // Find the smallest AST node whose span contains `pos`.
    use oxc::span::GetSpan;
    let mut best: Option<(u32, NodeId)> = None;
    for node in nodes.iter() {
        let sp = node.kind().span();
        if sp.start <= target && target < sp.end {
            let width = sp.end - sp.start;
            if best.map_or(true, |(w, _)| width < w) {
                best = Some((width, node.id()));
            }
        }
    }
    let Some((_, node_id)) = best else { return false };
    let mut scope_id = node_id_to_scope(nodes, scoping, node_id);
    while let Some(sid) = scope_id {
        if scoping
            .find_binding(sid, oxc::span::Ident::new_const(var_name))
            .is_some()
        {
            return true;
        }
        // Stop after the enclosing function scope so shadowing only applies
        // within the nearest function.
        if scoping.scope_flags(sid).contains(oxc::semantic::ScopeFlags::Function) {
            return false;
        }
        scope_id = scoping.scope_parent_id(sid);
    }
    false
}

fn node_id_to_scope(
    nodes: &oxc::semantic::AstNodes,
    scoping: &oxc::semantic::Scoping,
    mut id: NodeId,
) -> Option<oxc::semantic::ScopeId> {
    loop {
        if let Some(sid) = node_scope_id(nodes.kind(id)) {
            return Some(sid);
        }
        let parent = nodes.parent_id(id);
        if parent == id { return Some(scoping.root_scope_id()); }
        id = parent;
    }
}

fn node_scope_id(kind: AstKind<'_>) -> Option<oxc::semantic::ScopeId> {
    use oxc::ast::AstKind;
    match kind {
        AstKind::Program(p) => p.scope_id.get(),
        AstKind::BlockStatement(b) => b.scope_id.get(),
        AstKind::Function(f) => f.scope_id.get(),
        AstKind::ArrowFunctionExpression(a) => a.scope_id.get(),
        AstKind::CatchClause(c) => c.scope_id.get(),
        AstKind::ForStatement(f) => f.scope_id.get(),
        AstKind::ForInStatement(f) => f.scope_id.get(),
        AstKind::ForOfStatement(f) => f.scope_id.get(),
        AstKind::SwitchStatement(s) => s.scope_id.get(),
        AstKind::TSModuleDeclaration(m) => m.scope_id.get(),
        AstKind::Class(c) => c.scope_id.get(),
        _ => None,
    }
}
