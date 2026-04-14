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
        let is_svelte_module = ctx.is_svelte_module;
        for (sem, content, offset) in [
            (
                ctx.instance_semantic,
                ctx.ast.instance.as_ref().map(|s| s.content.as_str()),
                ctx.instance_content_offset as usize,
            ),
            (
                ctx.module_semantic,
                ctx.ast.module.as_ref().map(|s| s.content.as_str()),
                ctx.module_content_offset as usize,
            ),
        ] {
            let (Some(sem), Some(content)) = (sem, content) else { continue };
            if content.trim().is_empty() {
                continue;
            }
            self.check_script(ctx, sem, content, offset, is_svelte_module);
        }

        if !ctx.is_svelte_module {
            detect_chained_new_in_template(ctx);
        }
    }
}

impl PreferSvelteReactivity {
    fn check_script<'a>(
        &self,
        ctx: &mut LintContext<'a>,
        semantic: &'a oxc::semantic::Semantic<'a>,
        content: &str,
        content_offset: usize,
        is_svelte_module: bool,
    ) {
        use oxc::ast::ast::Expression;
        use oxc::ast::AstKind;
        use oxc::semantic::SymbolId;

        let scoping = semantic.scoping();
        let nodes = semantic.nodes();

        let shadowed = collect_shadowed_classes(content);
        let mut flagged_symbols = std::collections::HashSet::<SymbolId>::new();

        for ast_node in nodes.iter() {
            let AstKind::NewExpression(new_expr) = ast_node.kind() else { continue };
            let Expression::Identifier(callee) = &new_expr.callee else { continue };
            let class_name = callee.name.as_str();

            let Some(builtin) = BUILTIN_CLASSES.iter().find(|b| b.name == class_name) else { continue };
            if shadowed.contains(&class_name.to_string()) { continue; }

            if scoping.has_binding(callee.reference_id()) { continue; }
            if class_name == "URL" || class_name == "URLSearchParams" {
                if ast_node.scope_id() != scoping.root_scope_id() {
                    continue;
                }
            }

            let new_node_id = callee.node_id.get();
            let new_node_id = nodes.parent_id(new_node_id); // up to NewExpression
            if is_inside_state_call(nodes, new_node_id) { continue; }

            let new_span = || Span::new(
                (content_offset + new_expr.span.start as usize) as u32,
                (content_offset + new_expr.span.end as usize) as u32,
            );

            if is_svelte_module && is_inside_export(nodes, new_node_id) {
                ctx.diagnostic(mutable_class_msg(builtin), new_span());
                continue;
            }

            if is_directly_mutated(nodes, new_node_id, builtin) {
                ctx.diagnostic(mutable_class_msg(builtin), new_span());
                continue;
            }

            let (symbol_id, var_name_fallback) = find_target_symbol_or_name(nodes, scoping, new_node_id);
            let symbol_id = match symbol_id {
                Some(sid) => sid,
                None => {
                    if let Some(var_name) = &var_name_fallback {
                        if var_name.starts_with('$') { continue; }
                        if check_mutation_in(ctx.source, var_name, builtin, None) {
                            ctx.diagnostic(mutable_class_msg(builtin), new_span());
                        }
                    }
                    continue;
                }
            };

            if flagged_symbols.contains(&symbol_id) { continue; }
            let is_mut = is_symbol_mutated(scoping, nodes, symbol_id, builtin);
            let is_transitive_mut = if !is_mut {
                is_transitively_mutated(scoping, nodes, symbol_id, builtin)
            } else {
                false
            };

            let is_template_mut = !is_mut && !is_transitive_mut
                && scoping.symbol_scope_id(symbol_id) == scoping.root_scope_id()
                && {
                    let vn = scoping.symbol_name(symbol_id);
                    check_mutation_in(ctx.source, vn, builtin, Some(content))
                        || scoping.get_resolved_references(symbol_id).any(|ref_id| {
                            ref_id.is_read() && {
                                let (ds, _) = find_target_symbol_or_name(nodes, scoping, ref_id.node_id());
                                ds.is_some_and(|s| check_mutation_in(ctx.source, scoping.symbol_name(s), builtin, Some(content)))
                            }
                        })
                };

            if is_mut || is_transitive_mut || is_template_mut {
                flagged_symbols.insert(symbol_id);
                ctx.diagnostic(mutable_class_msg(builtin), new_span());
            }
        }
    }
}

fn mutable_class_msg(builtin: &BuiltinClass) -> String {
    format!("Found a mutable instance of the built-in {} class. Use {} instead.", builtin.name, builtin.svelte_name)
}

fn is_inside_export(nodes: &oxc::semantic::AstNodes, node_id: oxc::semantic::NodeId) -> bool {
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

fn is_directly_mutated(nodes: &oxc::semantic::AstNodes, new_expr_id: oxc::semantic::NodeId, builtin: &BuiltinClass) -> bool {
    use oxc::ast::AstKind;
    let mut current = new_expr_id;
    while matches!(nodes.kind(nodes.parent_id(current)),
        AstKind::ParenthesizedExpression(_) | AstKind::LogicalExpression(_) | AstKind::ConditionalExpression(_)) {
        current = nodes.parent_id(current);
    }
    let parent_id = nodes.parent_id(current);
    if let AstKind::StaticMemberExpression(member) = nodes.kind(parent_id) {
        let prop = member.property.name.as_str();
        let gp = nodes.parent_id(parent_id);
        (builtin.mutating_methods.contains(&prop) && matches!(nodes.kind(gp), AstKind::CallExpression(_)))
            || (builtin.mutating_props.contains(&prop) && matches!(nodes.kind(gp), AstKind::AssignmentExpression(_)))
    } else { false }
}

fn is_inside_state_call(nodes: &oxc::semantic::AstNodes, new_expr_id: oxc::semantic::NodeId) -> bool {
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

fn find_target_symbol_or_name(nodes: &oxc::semantic::AstNodes, scoping: &oxc::semantic::Scoping, node_id: oxc::semantic::NodeId) -> (Option<oxc::semantic::SymbolId>, Option<String>) {
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

fn is_symbol_mutated(scoping: &oxc::semantic::Scoping, nodes: &oxc::semantic::AstNodes, symbol_id: oxc::semantic::SymbolId, builtin: &BuiltinClass) -> bool {
    use oxc::ast::AstKind;
    use oxc::span::GetSpan;
    for reference in scoping.get_resolved_references(symbol_id) {
        if !reference.is_read() { continue; }
        let parent_id = nodes.parent_id(reference.node_id());
        if let AstKind::StaticMemberExpression(member) = nodes.kind(parent_id) {
            let prop = member.property.name.as_str();
            let gp = nodes.parent_id(parent_id);
            if builtin.mutating_methods.contains(&prop) && matches!(nodes.kind(gp), AstKind::CallExpression(_)) { return true; }
            if builtin.mutating_props.contains(&prop)
                && matches!(nodes.kind(gp), AstKind::AssignmentExpression(a) if member.span.start == a.left.span().start) { return true; }
        }
    }
    false
}

fn is_transitively_mutated(scoping: &oxc::semantic::Scoping, nodes: &oxc::semantic::AstNodes, symbol_id: oxc::semantic::SymbolId, builtin: &BuiltinClass) -> bool {
    use oxc::ast::AstKind;
    use oxc::ast::ast::BindingPattern;

    for reference in scoping.get_resolved_references(symbol_id) {
        if !reference.is_read() { continue; }
        for ancestor_id in nodes.ancestor_ids(reference.node_id()) {
            let target_sym = match nodes.kind(ancestor_id) {
                AstKind::VariableDeclarator(decl) => {
                    if let BindingPattern::BindingIdentifier(ident) = &decl.id { ident.symbol_id.get() } else { None }
                }
                AstKind::AssignmentExpression(assign) => {
                    if let oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(ident) = &assign.left {
                        ident.reference_id.get().and_then(|r| scoping.get_reference(r).symbol_id())
                    } else { None }
                }
                AstKind::ParenthesizedExpression(_) | AstKind::LogicalExpression(_)
                | AstKind::ConditionalExpression(_) | AstKind::SequenceExpression(_) => continue,
                _ => break,
            };
            if let Some(s) = target_sym {
                if s != symbol_id && is_symbol_mutated(scoping, nodes, s, builtin) { return true; }
            }
            break;
        }
    }
    false
}

fn check_mutation_in(text: &str, var_name: &str, builtin: &BuiltinClass, exclude: Option<&str>) -> bool {
    for method in builtin.mutating_methods {
        let pat = format!("{}.{}(", var_name, method);
        if find_word_boundary_pos(text, &pat).is_some() {
            if exclude.is_none_or(|ex| find_word_boundary_pos(ex, &pat).is_none()) { return true; }
        }
    }
    for prop in builtin.mutating_props {
        let pat = format!("{}.{} =", var_name, prop);
        if let Some(pos) = find_word_boundary_pos(text, &pat) {
            if !text[pos + pat.len()..].starts_with('=')
                && exclude.is_none_or(|ex| find_word_boundary_pos(ex, &pat).is_none()) { return true; }
        }
    }
    false
}

fn find_word_boundary_pos(content: &str, pat: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = content[start..].find(pat) {
        let abs = start + pos;
        if abs == 0 || !matches!(content.as_bytes()[abs - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') {
            return Some(abs);
        }
        start = abs + 1;
    }
    None
}

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

fn detect_chained_new_in_template(ctx: &mut LintContext) {
    let source = ctx.source;
    let scripts: Vec<_> = [&ctx.ast.instance, &ctx.ast.module].into_iter().flatten().collect();
    let script_ranges: Vec<_> = scripts.iter().map(|s| (s.span.start as usize, s.span.end as usize)).collect();
    let shadowed: Vec<_> = scripts.iter().flat_map(|s| collect_shadowed_classes(&s.content)).collect();

    for builtin in BUILTIN_CLASSES {
        if shadowed.iter().any(|s| s == builtin.name) { continue; }

        let new_pat = format!("new {}(", builtin.name);
        let mut search = 0;

        while let Some(rel) = source[search..].find(&new_pat) {
            let abs = search + rel;

            if abs > 0 && matches!(source.as_bytes()[abs - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') {
                search = abs + 1; continue;
            }

            if script_ranges.iter().any(|&(s, e)| abs >= s && abs < e) {
                search = abs + new_pat.len();
                continue;
            }

            let paren_start = abs + new_pat.len() - 1;
            let close = match find_matching_paren(source, paren_start) {
                Some(p) => p,
                None => { search = abs + 1; continue; }
            };

            let after = &source[close + 1..];
            let mut found_chained = false;
            if after.starts_with('.') {
                let method_start = &after[1..];
                for method in builtin.mutating_methods {
                    let call = format!("{}(", method);
                    if method_start.starts_with(&call) {
                        ctx.diagnostic(mutable_class_msg(builtin), Span::new(abs as u32, (close + 1) as u32));
                        found_chained = true;
                        break;
                    }
                }
            }

            if !found_chained && builtin.name != "URL" && builtin.name != "URLSearchParams" {
                if let Some(var_name) = extract_assigned_var(source, abs) {
                    if let Some((scope_start, scope_end)) = find_enclosing_brace_scope(source, abs) {
                        let scope_text = &source[scope_start..scope_end];
                        if has_mutating_call_in_scope(scope_text, &var_name, builtin) {
                            ctx.diagnostic(mutable_class_msg(builtin), Span::new(abs as u32, (close + 1) as u32));
                        }
                    }
                }
            }

            search = abs + 4;
        }
    }
}

fn extract_assigned_var(source: &str, new_pos: usize) -> Option<String> {
    let before = &source[..new_pos];
    let trimmed = before.trim_end();
    if !trimmed.ends_with('=') { return None; }
    let before_eq = trimmed[..trimmed.len() - 1].trim_end();
    if before_eq.ends_with('=') || before_eq.ends_with('!') || before_eq.ends_with('>') || before_eq.ends_with('<') {
        return None;
    }
    let bytes = before_eq.as_bytes();
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1].is_ascii_alphanumeric() || bytes[end - 1] == b'_' || bytes[end - 1] == b'$') {
        end -= 1;
    }
    let var_name = &before_eq[end..];
    if var_name.is_empty() { return None; }
    if matches!(var_name, "return" | "const" | "let" | "var" | "new" | "typeof" | "void" | "delete") {
        return None;
    }
    Some(var_name.to_string())
}

fn find_enclosing_brace_scope(source: &str, pos: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut i = pos;
    let mut scope_start = None;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 {
                    scope_start = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let scope_start = scope_start?;
    depth = 1;
    let mut j = scope_start + 1;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 { return Some((scope_start, j + 1)); }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

fn has_mutating_call_in_scope(scope_text: &str, var_name: &str, builtin: &BuiltinClass) -> bool {
    for method in builtin.mutating_methods {
        let pat = format!("{}.{}(", var_name, method);
        if scope_text.contains(&pat) { return true; }
    }
    for prop in builtin.mutating_props {
        let pat = format!("{}.{}", var_name, prop);
        for (pos, _) in scope_text.match_indices(&pat) {
            let after = &scope_text[pos + pat.len()..];
            let next = after.trim_start();
            if next.starts_with('=') && !next.starts_with("==") {
                return true;
            }
        }
    }
    false
}

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
