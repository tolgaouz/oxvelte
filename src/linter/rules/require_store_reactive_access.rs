//! `svelte/require-store-reactive-access` — require `$store` syntax for reactive access.
//! ⭐ Recommended 🔧 Fixable

use crate::linter::{parse_imports, walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart};
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
        let imports = parse_imports(content);

        let mut factory_names: HashSet<String> = HashSet::new();
        for (local, imported, module) in &imports {
            if module == "svelte/store" && STORE_FACTORIES.contains(&imported.as_str()) {
                factory_names.insert(local.clone());
            }
        }

        let mut store_vars_map: HashMap<String, bool> = HashMap::new();

        for line in content.lines() {
            let trimmed = line.trim();
            for (prefix, is_const) in &[("const ", true), ("let ", false)] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if name.is_empty() { continue; }
                    if let Some(eq) = rest.find('=') {
                        let init = rest[eq + 1..].trim();
                        let is_store = factory_names.iter().any(|f| init.starts_with(&format!("{}(", f)));
                        if is_store {
                            store_vars_map.insert(name.to_string(), *is_const);
                        }
                    }
                    if let Some(colon) = rest[name_end..].find(':') {
                        let type_start = name_end + colon + 1;
                        let type_end = rest[type_start..].find('=').map(|p| type_start + p).unwrap_or(rest.len());
                        let type_text = rest[type_start..type_end].trim();
                        if is_store_type(type_text) {
                            store_vars_map.insert(name.to_string(), *is_const);
                        }
                    }
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
                        if imported == "*" {
                            let store_exports = detect_store_exports(&module_content);
                            for export_name in &store_exports {
                                let qualified = format!("{}.{}", local, export_name);
                                store_vars_map.insert(qualified, true);
                            }
                            for line in module_content.lines() {
                                let trimmed = line.trim();
                                if trimmed.starts_with("export const ") || trimmed.starts_with("export let ") {
                                    let is_const = trimmed.starts_with("export const ");
                                    let after = if is_const { &trimmed[14..] } else { &trimmed[11..] };
                                    let name_end = after.find(|c: char| !c.is_alphanumeric() && c != '_')
                                        .unwrap_or(after.len());
                                    let name = &after[..name_end];
                                    if store_exports.contains(&name.to_string()) {
                                        let qualified = format!("{}.{}", local, name);
                                        store_vars_map.insert(qualified, is_const);
                                    }
                                }
                            }
                        } else {
                            let store_exports = detect_store_exports(&module_content);
                            if store_exports.contains(imported) {
                                let is_const = module_content.contains(&format!("export const {}", imported))
                                    || module_content.contains(&format!("const {} =", imported));
                                store_vars_map.insert(local.clone(), is_const);
                            }
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
        for var in &store_vars {
            let raw_interp = format!("${{{}}}", var);
            let reactive_interp = format!("${{${}}}", var);
            for (pos, _) in content.match_indices(&raw_interp) {
                if content[pos..].starts_with(&reactive_interp) { continue; }
                if pos > 0 && content.as_bytes()[pos - 1] == b'$' { continue; }
                let src_pos = content_offset + pos;
                ctx.diagnostic(
                    RAW_STORE_MSG,
                    oxc::span::Span::new(src_pos as u32, (src_pos + raw_interp.len()) as u32),
                );
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

                let consistent_context =
                    before.ends_with("typeof") || before.ends_with("typeof ")
                    || before.ends_with('!')
                    || after_text.starts_with("==") || after_text.starts_with("!=")
                    || after_text.starts_with("&&") || after_text.starts_with("||") || after_text.starts_with("??")
                    || (after_text.starts_with('?') && !after_text.starts_with("?."))
                    || before.ends_with("await") || before.ends_with("await ");

                if consistent_context {
                    if is_const_store(var) {
                        let src_pos = content_offset + pos;
                        ctx.diagnostic(
                            RAW_STORE_MSG,
                            oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                        );
                    }
                    continue;
                }

                let before_trimmed = before.trim_end();
                let in_for_in_of = before_trimmed.ends_with(" in") || before_trimmed.ends_with(" of")
                    || before_trimmed.ends_with("\tin") || before_trimmed.ends_with("\tof");

                if in_for_in_of {
                    let src_pos = content_offset + pos;
                    ctx.diagnostic(
                        RAW_STORE_MSG,
                        oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                    );
                    continue;
                }

                if before.ends_with('(') {
                    let kw_before = before[..before.len()-1].trim_end();
                    if kw_before.ends_with("if") {
                        if is_const_store(var) {
                            let src_pos = content_offset + pos;
                            ctx.diagnostic(
                                RAW_STORE_MSG,
                                oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                            );
                        }
                        continue;
                    }
                    if kw_before.ends_with("switch") || kw_before.ends_with("while") {
                        let src_pos = content_offset + pos;
                        ctx.diagnostic(
                            RAW_STORE_MSG,
                            oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                        );
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

                let src_pos = content_offset + pos;
                ctx.diagnostic(
                    RAW_STORE_MSG,
                    oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                );
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
                                if dir_name == var.as_str() && is_shorthand {
                                    ctx.diagnostic(
                                        RAW_STORE_MSG,
                                        *span,
                                    );
                                    continue;
                                }
                                if let Some(eq) = region.find('=') {
                                    let val = &region[eq+1..];
                                    if let Some(open) = val.find('{') {
                                        if let Some(close) = val.find('}') {
                                            let expr = val[open+1..close].trim();
                                            if expr == var.as_str() && !expr.starts_with('$') {
                                                ctx.diagnostic(
                                                    RAW_STORE_MSG,
                                                    *span,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Attribute::Spread { span } = attr {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            for var in &store_vars_clone {
                                let has_raw_ref = has_word_boundary_match(region, var);
                                if has_raw_ref && !region.contains(&format!("${}", var)) {
                                    ctx.diagnostic(
                                        RAW_STORE_MSG,
                                        *span,
                                    );
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

/// Check if a TypeScript type annotation text indicates a store type.
fn is_store_type(type_text: &str) -> bool {
    const STORE_TYPES: &[&str] = &["Writable", "Readable", "Derived"];
    let text = type_text.trim();
    for st in STORE_TYPES {
        if text.starts_with(st) || text.contains(&format!("| {}", st)) || text.contains(&format!("{} |", st))
            || text.contains(&format!("| {}<", st)) || text.starts_with(&format!("{}<", st)) {
            return true;
        }
    }
    false
}

/// Resolve a relative module path to a file, trying .ts and .js extensions.
fn resolve_module_file(dir: &std::path::Path, module: &str) -> Option<String> {
    for ext in &["", ".ts", ".js", ".d.ts"] {
        let path = dir.join(format!("{}{}", module, ext));
        if let Ok(content) = std::fs::read_to_string(&path) {
            return Some(content);
        }
    }
    None
}

/// Detect which exports from a module file are stores.
/// Returns a set of export names that are stores.
fn detect_store_exports(content: &str) -> HashSet<String> {
    let mut stores = HashSet::new();
    let imports = crate::linter::parse_imports(content);

    let mut factory_names: HashSet<String> = HashSet::new();
    for (local, imported, module) in &imports {
        if module == "svelte/store" && STORE_FACTORIES.contains(&imported.as_str()) {
            factory_names.insert(local.clone());
        }
    }

    let mut store_type_names: HashSet<String> = HashSet::new();
    for (local, imported, module) in &imports {
        if module == "svelte/store" {
            match imported.as_str() {
                "Writable" | "Readable" | "Derived" => { store_type_names.insert(local.clone()); }
                _ => {}
            }
        }
    }

    for line in content.lines() {
        let trimmed = line.trim();
        for prefix in &["export const ", "export let "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                let name = &rest[..name_end];
                if name.is_empty() { continue; }

                if let Some(eq) = rest.find('=') {
                    let init = rest[eq + 1..].trim();
                    if factory_names.iter().any(|f| init.starts_with(&format!("{}(", f))) {
                        stores.insert(name.to_string());
                        continue;
                    }
                    if init.starts_with("derived(") {
                        stores.insert(name.to_string());
                        continue;
                    }
                }

                if let Some(colon) = rest[name_end..].find(':') {
                    let type_start = name_end + colon + 1;
                    let type_end = rest[type_start..].find('=').map(|p| type_start + p).unwrap_or(rest.len());
                    let type_text = rest[type_start..type_end].trim();
                    if is_store_type(type_text) || store_type_names.iter().any(|t| type_text.contains(t)) {
                        stores.insert(name.to_string());
                        continue;
                    }
                }

                if let Some(colon) = rest[name_end..].find(':') {
                    let type_start = name_end + colon + 1;
                    let type_text = rest[type_start..].trim().trim_end_matches(';');
                    let extends_store = content.contains(&format!("interface {} extends", type_text))
                        && (content.contains(&format!("{} extends Writable", type_text))
                            || content.contains(&format!("{} extends Readable", type_text))
                            || content.contains(&format!("{} extends Derived", type_text)));
                    if extends_store {
                        stores.insert(name.to_string());
                    }
                }
            }
        }
    }

    for line in content.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("export const ") || trimmed.starts_with("export let "))
            && trimmed.contains('{')
        {
            let is_const = trimmed.starts_with("export const ");
            let prefix_len = if is_const { 14 } else { 11 };
            let rest = &trimmed[prefix_len..];
            let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let name = &rest[..name_end];
            if name.is_empty() || stores.contains(name) { continue; }
            let has_store_ref = stores.iter().any(|s| trimmed.contains(s.as_str()));
            if has_store_ref {
                stores.insert(name.to_string());
            }
        }
    }

    stores
}
