//! `svelte/require-store-reactive-access` — require `$store` syntax for reactive access.
//! ⭐ Recommended 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart};
use oxc::ast::ast::{
    BindingPattern, Declaration, Expression, ImportDeclarationSpecifier, ModuleExportName,
    Statement, TSType, TSTypeName, VariableDeclaration, VariableDeclarationKind,
};
use oxc::ast::AstKind;
use std::collections::{HashSet, HashMap};

const STORE_FACTORIES: &[&str] = &["writable", "readable", "derived"];
const RAW_STORE_MSG: &str = "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.";

fn check_expr_for_raw_store(
    expr: &str, span: oxc::span::Span,
    store_vars: &HashSet<String>, ctx: &mut LintContext<'_>,
) {
    let expr = expr.trim();
    for var in store_vars {
        if expr == var
            || expr.starts_with(&format!("{}.", var))
            || expr.starts_with(&format!("{}[", var))
            || expr.starts_with(&format!("{}(", var))
        {
            if !expr.contains(&format!("${}", var))
                && !expr.contains(&format!("get({})", var))
            {
                ctx.diagnostic(
                    RAW_STORE_MSG,
                    span,
                );
            }
        }
    }
}

pub struct RequireStoreReactiveAccess;

impl Rule for RequireStoreReactiveAccess {
    fn name(&self) -> &'static str {
        "svelte/require-store-reactive-access"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let script = match &ctx.ast.instance { Some(s) => s, None => return };
        let content = &script.content;
        let imports = match ctx.instance_semantic {
            Some(sem) => collect_imports(sem.nodes().program().body.as_slice()),
            None => Vec::new(),
        };

        let mut factory_names: HashSet<String> = HashSet::new();
        for (local, imported, module) in &imports {
            if module == "svelte/store" && STORE_FACTORIES.contains(&imported.as_str()) {
                factory_names.insert(local.clone());
            }
        }

        // Walk top-level `const`/`let` declarations in the instance script and
        // mark a binding as a store when its initializer calls one of the
        // factory-name imports (`writable`/`readable`/`derived`) or when it
        // has a TS type annotation whose reference is a store type.
        let mut store_vars_map: HashMap<String, bool> = HashMap::new();
        if let Some(sem) = ctx.instance_semantic {
            for stmt in &sem.nodes().program().body {
                match stmt {
                    Statement::VariableDeclaration(vd) => {
                        collect_store_vars_from_decl(vd, &factory_names, &mut store_vars_map);
                    }
                    Statement::ExportNamedDeclaration(exp) => {
                        if let Some(Declaration::VariableDeclaration(vd)) = &exp.declaration {
                            collect_store_vars_from_decl(vd, &factory_names, &mut store_vars_map);
                        }
                    }
                    _ => {}
                }
            }
        }

        const KNOWN_STORE_PACKAGES: &[&str] = &["svelte-i18n"];
        for (local, _imported, module) in &imports {
            if KNOWN_STORE_PACKAGES.iter().any(|pkg| module == *pkg) {
                if local != "*" {
                    store_vars_map.insert(local.clone(), true);
                }
            }
        }

        if let Some(file_path) = &ctx.file_path {
            for (local, imported, module) in &imports {
                if module.starts_with('.') && module != "svelte/store" {
                    let dir = std::path::Path::new(file_path.as_str()).parent()
                        .unwrap_or(std::path::Path::new("."));
                    let resolved = resolve_module_file(dir, module);
                    if let Some(module_content) = resolved {
                        let store_exports = detect_store_exports(&module_content);
                        if imported == "*" {
                            for (name, is_const) in &store_exports {
                                let qualified = format!("{}.{}", local, name);
                                store_vars_map.insert(qualified, *is_const);
                            }
                        } else if let Some(&is_const) = store_exports.get(imported) {
                            store_vars_map.insert(local.clone(), is_const);
                        }
                    }
                }
            }
        }

        if store_vars_map.is_empty() { return; }
        let store_vars: HashSet<String> = store_vars_map.keys().cloned().collect();
        let const_store_vars: HashSet<String> = store_vars_map.iter()
            .filter(|(_, is_const)| **is_const)
            .map(|(name, _)| name.clone())
            .collect();

        let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| script.span.start as usize + p + 1)
            .unwrap_or(script.span.start as usize);
        // `${storeName}` or `${ns.store}` inside a template literal is a raw
        // store access. Walk TemplateLiteral nodes directly — the expression
        // inside each `${…}` is already an oxc Expression, so we don't need
        // to re-find `${…}` pairs in the source.
        if let Some(sem) = ctx.instance_semantic {
            for node in sem.nodes().iter() {
                let AstKind::TemplateLiteral(tl) = node.kind() else { continue };
                for (i, expr) in tl.expressions.iter().enumerate() {
                    let (name, span) = match expr {
                        Expression::Identifier(id) => (id.name.as_str().to_string(), id.span),
                        Expression::StaticMemberExpression(m) => match &m.object {
                            Expression::Identifier(obj) =>
                                (format!("{}.{}", obj.name, m.property.name), m.span),
                            _ => continue,
                        },
                        _ => continue,
                    };
                    if !store_vars.contains(&name) { continue; }
                    // Mirror the old "`$` right before `${...}`" suppression:
                    // if the quasi preceding this expression ends with a `$`,
                    // treat it as escaped / intentional raw access.
                    if tl.quasis.get(i).is_some_and(|q| q.value.raw.as_str().ends_with('$')) {
                        continue;
                    }
                    let sp = content_offset as u32 + span.start;
                    let end = content_offset as u32 + span.end;
                    ctx.diagnostic(RAW_STORE_MSG, oxc::span::Span::new(sp, end));
                }
            }
        }

        let is_const_store = |v: &str| store_vars_map.get(v).copied() == Some(true);
        for var in &store_vars {
            for (pos, _) in content.match_indices(var.as_str()) {
                if pos > 0 {
                    let p = content.as_bytes()[pos - 1];
                    if p.is_ascii_alphanumeric() || p == b'_' || p == b'$' { continue; }
                    if p == b'.' {
                        let is_spread = pos >= 3 && &content[pos-3..pos] == "...";
                        if !is_spread { continue; }
                    }
                    if p == b'\'' || p == b'"' { continue; }
                }
                let after = pos + var.len();
                if after < content.len() {
                    let a = content.as_bytes()[after];
                    if a.is_ascii_alphanumeric() || a == b'_' { continue; }
                    if a == b'\'' || a == b'"' { continue; }
                }
                if pos > 0 && content.as_bytes()[pos - 1] == b'$' { continue; }
                let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line = content[line_start..].trim_start();
                if line.starts_with("const ") || line.starts_with("let ") || line.starts_with("import ") || line.starts_with("//") { continue; }
                if line.starts_with("// eslint-") { continue; }
                let before = content[..pos].trim_end();
                if before.ends_with("get(") { continue; }
                let line_end = content[pos..].find('\n').map(|p| pos + p).unwrap_or(content.len());
                let full_line = &content[line_start..line_end];
                if full_line.contains(&format!("${}", var)) { continue; }

                let after_text = if after < content.len() { content[after..].trim_start() } else { "" };

                if after_text.starts_with('.') || after_text.starts_with("?.") { continue; }

                if after_text.starts_with(':') && !after_text.starts_with("::") {
                    let before_check = before.trim_end();
                    if before_check.ends_with('{') || before_check.ends_with(',') || before_check.ends_with('\n') {
                        continue;
                    }
                }

                if after_text.starts_with('=') && !after_text.starts_with("==") {
                    if after_text.starts_with("= ") || after_text.starts_with("=\n") || after_text.starts_with("=\t") {
                        continue;
                    }
                }

                if before.ends_with('=') && !before.ends_with("!=") && !before.ends_with("==")
                    && !before.ends_with("+=") && !before.ends_with("-=")
                    && !before.ends_with("*=") && !before.ends_with("/=")
                    && !after_text.starts_with('(') { continue; }

                if before.ends_with(':') && !after_text.starts_with(']') { continue; }

                let consistent_context = ["typeof", "typeof ", "!", "await", "await "].iter().any(|p| before.ends_with(p))
                    || ["==", "!=", "&&", "||", "??"].iter().any(|p| after_text.starts_with(p))
                    || (after_text.starts_with('?') && !after_text.starts_with("?."));

                if consistent_context {
                    if is_const_store(var) {
                        let sp = content_offset + pos;
                        ctx.diagnostic(RAW_STORE_MSG, oxc::span::Span::new(sp as u32, (sp + var.len()) as u32));
                    }
                    continue;
                }

                let before_trimmed = before.trim_end();
                let in_for_in_of = before_trimmed.ends_with(" in") || before_trimmed.ends_with(" of")
                    || before_trimmed.ends_with("\tin") || before_trimmed.ends_with("\tof");

                if in_for_in_of {
                    let sp = content_offset + pos;
                    ctx.diagnostic(RAW_STORE_MSG, oxc::span::Span::new(sp as u32, (sp + var.len()) as u32));
                    continue;
                }

                if before.ends_with('(') {
                    let kw_before = before[..before.len()-1].trim_end();
                    if kw_before.ends_with("if") || kw_before.ends_with("switch") || kw_before.ends_with("while") {
                        if !kw_before.ends_with("if") || is_const_store(var) {
                            let sp = content_offset + pos;
                            ctx.diagnostic(RAW_STORE_MSG, oxc::span::Span::new(sp as u32, (sp + var.len()) as u32));
                        }
                        continue;
                    }
                }

                if (before.ends_with('(') || before.ends_with(", ") || before.ends_with(','))
                    && !after_text.starts_with('`') && !after_text.starts_with('(')
                    && !before.ends_with("import(") { continue; }

                if after_text.starts_with('(') {
                    let line_trimmed = line.trim_start();
                    let is_method_def = line_trimmed.starts_with(var.as_str())
                        && !line.contains('=') && !line.contains("$:");
                    if is_method_def { continue; }
                }

                let in_computed_key = before.ends_with('[');
                let in_import = before.ends_with("import(");
                let in_spread = before.ends_with("...");
                if (after_text.starts_with(')') || after_text.starts_with(',') || after_text.starts_with(';'))
                    && !in_import && !in_spread {
                    continue;
                }
                if after_text.starts_with(']') && !in_computed_key {
                    continue;
                }

                let sp = content_offset + pos;
                ctx.diagnostic(RAW_STORE_MSG, oxc::span::Span::new(sp as u32, (sp + var.len()) as u32));
            }
        }

        let store_vars_clone = store_vars.clone();
        let const_store_vars_clone = const_store_vars.clone();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            match node {
                TemplateNode::MustacheTag(tag) => {
                    check_expr_for_raw_store(&tag.expression, tag.span, &store_vars_clone, ctx);
                }
                TemplateNode::IfBlock(block) => {
                    check_expr_for_raw_store(&block.test, block.span, &const_store_vars_clone, ctx);
                }
                TemplateNode::EachBlock(block) => {
                    check_expr_for_raw_store(&block.expression, block.span, &store_vars_clone, ctx);
                }
                TemplateNode::Element(el) => {
                    let is_component = el.name.chars().next().map_or(false, |c| c.is_uppercase())
                        || el.name.contains('.');
                    for attr in &el.attributes {
                        if let Attribute::NormalAttribute { name, value, span, .. } = attr {
                            let is_css_var = name.starts_with("--");
                            match value {
                                AttributeValue::Expression(expr) => {
                                    if !is_component || is_css_var {
                                        check_expr_for_raw_store(expr, *span, &store_vars_clone, ctx);
                                    }
                                }
                                AttributeValue::Concat(parts) => {
                                    for part in parts {
                                        if let AttributeValuePart::Expression(expr) = part {
                                            check_expr_for_raw_store(expr, *span, &store_vars_clone, ctx);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        if let Attribute::Directive { kind, name: dir_name, span, .. } = attr {
                            if is_component {
                                let is_bind_this = matches!(kind, crate::ast::DirectiveKind::Binding)
                                    && dir_name == "this";
                                if !is_bind_this { continue; }
                            }
                            let check_vars = if matches!(kind, crate::ast::DirectiveKind::Class) {
                                &const_store_vars_clone
                            } else {
                                &store_vars_clone
                            };
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            let is_shorthand = !region.contains('=');
                            for var in check_vars {
                                let flag = if dir_name == var.as_str() && is_shorthand { true }
                                else if let Some(eq) = region.find('=') {
                                    let val = &region[eq+1..];
                                    val.find('{').and_then(|o| val.find('}').map(|c| val[o+1..c].trim()))
                                        .is_some_and(|expr| expr == var.as_str() && !expr.starts_with('$'))
                                } else { false };
                                if flag { ctx.diagnostic(RAW_STORE_MSG, *span); }
                            }
                        }
                        if let Attribute::Spread { span } = attr {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            for var in &store_vars_clone {
                                if has_word_boundary_match(region, var) && !region.contains(&format!("${}", var)) {
                                    ctx.diagnostic(RAW_STORE_MSG, *span);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        });
    }
}

/// Record each identifier binding from a VariableDeclaration as a store when
/// the declarator's initializer is `factory(...)` or the declarator has a
/// store-typed annotation. The declaration's kind determines whether the
/// resulting map entry is `true` (const) or `false` (let/var).
fn collect_store_vars_from_decl(
    vd: &VariableDeclaration<'_>,
    factory_names: &HashSet<String>,
    out: &mut HashMap<String, bool>,
) {
    let is_const = matches!(
        vd.kind,
        VariableDeclarationKind::Const
            | VariableDeclarationKind::Using
            | VariableDeclarationKind::AwaitUsing
    );
    for d in &vd.declarations {
        let Some(name) = binding_identifier_name(&d.id) else { continue };
        let init_is_factory = d.init.as_ref()
            .map(|e| expression_calls_factory(e, factory_names))
            .unwrap_or(false);
        let typed_as_store = d.type_annotation
            .as_ref()
            .map(|ta| ts_type_is_store(&ta.type_annotation))
            .unwrap_or(false);
        if init_is_factory || typed_as_store {
            out.insert(name.to_string(), is_const);
        }
    }
}

fn binding_identifier_name<'a>(pat: &'a BindingPattern<'a>) -> Option<&'a str> {
    match pat {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// True iff the expression is a direct `name(...)` call whose callee matches
/// one of the known store-factory names.
fn expression_calls_factory(expr: &Expression<'_>, factory_names: &HashSet<String>) -> bool {
    let Expression::CallExpression(call) = expr else { return false };
    let Expression::Identifier(id) = &call.callee else { return false };
    factory_names.contains(id.name.as_str())
}

/// True iff the TS type names a store (directly, or as a member of a union).
fn ts_type_is_store(ty: &TSType<'_>) -> bool {
    match ty {
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else { return false };
            matches!(id.name.as_str(), "Writable" | "Readable" | "Derived")
        }
        TSType::TSUnionType(u) => u.types.iter().any(ts_type_is_store),
        TSType::TSParenthesizedType(p) => ts_type_is_store(&p.type_annotation),
        _ => false,
    }
}

/// Collect `(local, imported, module)` triples from top-level `ImportDeclaration`
/// statements. `imported` is `"default"` for default imports and `"*"` for
/// namespace imports, matching the legacy `parse_imports` shape.
fn collect_imports(body: &[Statement<'_>]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for stmt in body {
        let Statement::ImportDeclaration(imp) = stmt else { continue };
        let source = imp.source.value.as_str().to_string();
        let Some(specs) = &imp.specifiers else { continue };
        for spec in specs {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let imported = match &s.imported {
                        ModuleExportName::IdentifierName(n) => n.name.as_str().to_string(),
                        ModuleExportName::IdentifierReference(n) => n.name.as_str().to_string(),
                        ModuleExportName::StringLiteral(l) => l.value.as_str().to_string(),
                    };
                    out.push((s.local.name.as_str().to_string(), imported, source.clone()));
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    out.push((s.local.name.as_str().to_string(), "default".to_string(), source.clone()));
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    out.push((s.local.name.as_str().to_string(), "*".to_string(), source.clone()));
                }
            }
        }
    }
    out
}

fn has_word_boundary_match(text: &str, word: &str) -> bool {
    for (pos, _) in text.match_indices(word) {
        let before_ok = pos == 0 || {
            let p = text.as_bytes()[pos - 1];
            !p.is_ascii_alphanumeric() && p != b'_' && p != b'$'
        };
        let after_ok = pos + word.len() >= text.len() || {
            let a = text.as_bytes()[pos + word.len()];
            !a.is_ascii_alphanumeric() && a != b'_'
        };
        if before_ok && after_ok { return true; }
    }
    false
}

fn resolve_module_file(dir: &std::path::Path, module: &str) -> Option<String> {
    ["", ".ts", ".js", ".d.ts"].iter()
        .find_map(|ext| std::fs::read_to_string(dir.join(format!("{}{}", module, ext))).ok())
}

fn detect_store_exports(content: &str) -> HashMap<String, bool> {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, content, SourceType::ts()).parse();
    let body = parsed.program.body.as_slice();

    // Collect factory-name and store-type aliases from `svelte/store` imports.
    let (mut factory_names, mut store_type_names): (HashSet<String>, HashSet<String>) =
        (HashSet::new(), HashSet::new());
    for stmt in body {
        let Statement::ImportDeclaration(imp) = stmt else { continue };
        if imp.source.value != "svelte/store" { continue; }
        let Some(specs) = &imp.specifiers else { continue };
        for spec in specs {
            let ImportDeclarationSpecifier::ImportSpecifier(s) = spec else { continue };
            let imported = match &s.imported {
                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                ModuleExportName::StringLiteral(l) => l.value.as_str(),
            };
            let local = s.local.name.as_str().to_string();
            if STORE_FACTORIES.contains(&imported) { factory_names.insert(local.clone()); }
            if matches!(imported, "Writable" | "Readable" | "Derived") {
                store_type_names.insert(local);
            }
        }
    }

    // Index interfaces that extend a store type, so type annotations like
    // `MyStore` (where `interface MyStore extends Writable<T>`) are recognised.
    let mut store_interfaces: HashSet<String> = HashSet::new();
    for stmt in body {
        if let Statement::TSInterfaceDeclaration(iface) = stmt {
            let extends_store = iface.extends.iter().any(|h| {
                let Expression::Identifier(id) = &h.expression else { return false };
                matches!(id.name.as_str(), "Writable" | "Readable" | "Derived")
                    || store_type_names.contains(id.name.as_str())
            });
            if extends_store { store_interfaces.insert(iface.id.name.as_str().to_string()); }
        }
    }

    // First pass: exported VariableDeclarations whose init is a factory call
    // or whose binding has a store-typed annotation. Record (name, is_const)
    // using the enclosing VariableDeclaration's `kind`.
    let mut stores: HashMap<String, bool> = HashMap::new();
    // Each entry is (declarator, is_const) so pass 2 can propagate is_const.
    let mut exported_decls: Vec<(&oxc::ast::ast::VariableDeclarator, bool)> = Vec::new();
    for stmt in body {
        let Statement::ExportNamedDeclaration(exp) = stmt else { continue };
        let Some(Declaration::VariableDeclaration(vd)) = &exp.declaration else { continue };
        let is_const = matches!(
            vd.kind,
            VariableDeclarationKind::Const
                | VariableDeclarationKind::Using
                | VariableDeclarationKind::AwaitUsing
        );
        for d in &vd.declarations {
            exported_decls.push((d, is_const));
            let Some(name) = binding_identifier_name(&d.id) else { continue };
            let init_store = d.init.as_ref().map(|e| {
                if expression_calls_factory(e, &factory_names) { return true; }
                // `derived(...)` from svelte/store is always a store; the
                // import check would add it to `factory_names`, but the
                // legacy rule also accepts the bare `derived(` form.
                if let Expression::CallExpression(c) = e {
                    if let Expression::Identifier(id) = &c.callee {
                        if id.name.as_str() == "derived" { return true; }
                    }
                }
                false
            }).unwrap_or(false);
            let typed_store = d.type_annotation.as_ref().map(|ta| {
                ts_type_is_store(&ta.type_annotation)
                    || ts_type_references_names(&ta.type_annotation, &store_type_names)
                    || ts_type_references_names(&ta.type_annotation, &store_interfaces)
            }).unwrap_or(false);
            if init_store || typed_store { stores.insert(name.to_string(), is_const); }
        }
    }

    // Second pass: propagate through exports whose init references a store
    // already in the set (e.g. `export const doubled = derived(counter, ...)`
    // where `counter` was detected in pass 1). Iterate until a fixed point.
    loop {
        let mut added = false;
        let names: HashSet<String> = stores.keys().cloned().collect();
        for (d, is_const) in &exported_decls {
            let Some(name) = binding_identifier_name(&d.id) else { continue };
            if stores.contains_key(name) { continue; }
            let Some(init) = &d.init else { continue };
            if expression_references_any(init, &names) {
                stores.insert(name.to_string(), *is_const);
                added = true;
            }
        }
        if !added { break; }
    }

    stores
}

/// True iff `ty` is a `TSTypeReference` (possibly nested in a union/paren)
/// whose outer identifier is in `names`.
fn ts_type_references_names(ty: &TSType<'_>, names: &HashSet<String>) -> bool {
    match ty {
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else { return false };
            names.contains(id.name.as_str())
        }
        TSType::TSUnionType(u) => u.types.iter().any(|t| ts_type_references_names(t, names)),
        TSType::TSParenthesizedType(p) => ts_type_references_names(&p.type_annotation, names),
        _ => false,
    }
}

/// True iff the expression references (by identifier) any of the names in
/// `candidates`. Walks into CallExpression arguments / callee, MemberExpression
/// bases, ArrowFunctionExpression bodies, and a few other common forms.
fn expression_references_any(expr: &Expression<'_>, candidates: &HashSet<String>) -> bool {
    match expr {
        Expression::Identifier(id) => candidates.contains(id.name.as_str()),
        Expression::CallExpression(c) => {
            expression_references_any(&c.callee, candidates)
                || c.arguments.iter().any(|a| match a {
                    oxc::ast::ast::Argument::SpreadElement(s) =>
                        expression_references_any(&s.argument, candidates),
                    other => other.as_expression()
                        .map(|e| expression_references_any(e, candidates))
                        .unwrap_or(false),
                })
        }
        Expression::StaticMemberExpression(m) => expression_references_any(&m.object, candidates),
        Expression::ComputedMemberExpression(m) =>
            expression_references_any(&m.object, candidates)
                || expression_references_any(&m.expression, candidates),
        Expression::ParenthesizedExpression(p) => expression_references_any(&p.expression, candidates),
        Expression::ConditionalExpression(c) =>
            expression_references_any(&c.test, candidates)
                || expression_references_any(&c.consequent, candidates)
                || expression_references_any(&c.alternate, candidates),
        Expression::ChainExpression(c) => match &c.expression {
            oxc::ast::ast::ChainElement::CallExpression(cc) =>
                expression_references_any(&cc.callee, candidates),
            oxc::ast::ast::ChainElement::StaticMemberExpression(m) =>
                expression_references_any(&m.object, candidates),
            oxc::ast::ast::ChainElement::ComputedMemberExpression(m) =>
                expression_references_any(&m.object, candidates),
            _ => false,
        },
        Expression::ArrayExpression(a) => a.elements.iter().any(|e| match e {
            oxc::ast::ast::ArrayExpressionElement::SpreadElement(s) =>
                expression_references_any(&s.argument, candidates),
            oxc::ast::ast::ArrayExpressionElement::Elision(_) => false,
            other => other.as_expression()
                .map(|e| expression_references_any(e, candidates))
                .unwrap_or(false),
        }),
        _ => false,
    }
}
