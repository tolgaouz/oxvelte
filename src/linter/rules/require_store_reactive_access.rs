//! `svelte/require-store-reactive-access` — require `$store` syntax for reactive access.
//! ⭐ Recommended 🔧 Fixable

use crate::linter::{parse_imports, walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart};
use std::collections::HashSet;

const STORE_FACTORIES: &[&str] = &["writable", "readable", "derived"];

fn check_expr_for_raw_store(
    expr: &str, span: oxc::span::Span,
    store_vars: &HashSet<String>, ctx: &mut LintContext<'_>,
) {
    let expr = expr.trim();
    for var in store_vars {
        if expr == var
            || expr.starts_with(&format!("{}.", var))
            || expr.starts_with(&format!("{}[", var))
        {
            if !expr.contains(&format!("${}", var))
                && !expr.contains(&format!("get({})", var))
            {
                ctx.diagnostic(
                    "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
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

        // Find local names for store factory functions
        let mut factory_names: HashSet<String> = HashSet::new();
        for (local, imported, module) in &imports {
            if module == "svelte/store" && STORE_FACTORIES.contains(&imported.as_str()) {
                factory_names.insert(local.clone());
            }
        }
        if factory_names.is_empty() { return; }

        // Find variables assigned from store factories: const x = writable(...)
        let mut store_vars: HashSet<String> = HashSet::new();
        for line in content.lines() {
            let trimmed = line.trim();
            for prefix in &["const ", "let "] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if name.is_empty() { continue; }
                    // Check if initialized with a store factory
                    if let Some(eq) = rest.find('=') {
                        let init = rest[eq + 1..].trim();
                        let is_store = factory_names.iter().any(|f| init.starts_with(&format!("{}(", f)));
                        if is_store {
                            store_vars.insert(name.to_string());
                        }
                    }
                }
            }
        }

        if store_vars.is_empty() { return; }

        // Check script for raw store references in template literal interpolations
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
                    "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                    oxc::span::Span::new(src_pos as u32, (src_pos + raw_interp.len()) as u32),
                );
            }
        }

        // Script-level: check for operator patterns on raw store references.
        // Based on vendor eslint-plugin-svelte implementation:
        // - UpdateExpression (store++, --store) → forbidden
        // - UnaryExpression (- + ~ store) → forbidden, but !store and typeof store → allowed
        // - BinaryExpression (store + x, store * x) → forbidden, but ==, !=, ===, !== → allowed
        // - Compound assignment (store += x) → forbidden, but store = x → allowed
        // - LogicalExpression (store && x) → allowed
        // - Property access (store.subscribe) → allowed
        for var in &store_vars {
            for (pos, _) in content.match_indices(var.as_str()) {
                // Word boundary
                if pos > 0 {
                    let p = content.as_bytes()[pos - 1];
                    if p.is_ascii_alphanumeric() || p == b'_' || p == b'$' || p == b'.' { continue; }
                }
                let after = pos + var.len();
                if after < content.len() {
                    let a = content.as_bytes()[after];
                    if a.is_ascii_alphanumeric() || a == b'_' { continue; }
                }
                // Skip if preceded by $ (reactive access)
                if pos > 0 && content.as_bytes()[pos - 1] == b'$' { continue; }
                // Skip declarations and imports
                let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line = content[line_start..].trim_start();
                if line.starts_with("const ") || line.starts_with("let ") || line.starts_with("import ") || line.starts_with("//") { continue; }
                // Skip get() call
                let before = content[..pos].trim_end();
                if before.ends_with("get(") { continue; }
                // Skip if $var on same line
                let line_end = content[pos..].find('\n').map(|p| pos + p).unwrap_or(content.len());
                let full_line = &content[line_start..line_end];
                if full_line.contains(&format!("${}", var)) { continue; }

                let after_text = if after < content.len() { content[after..].trim_start() } else { "" };

                // ALLOWED patterns:
                // typeof store, !store → skip
                if before.ends_with("typeof") || before.ends_with("typeof ") { continue; }
                if before.ends_with('!') { continue; }
                // Comparison: store == x, store != x, store === x, store !== x → skip
                if after_text.starts_with("==") || after_text.starts_with("!=") { continue; }
                // Logical: store && x, store || x, store ?? x → skip
                if after_text.starts_with("&&") || after_text.starts_with("||") || after_text.starts_with("??") { continue; }
                // Ternary condition: store ? x : y → skip
                if after_text.starts_with('?') && !after_text.starts_with("?.") { continue; }
                // Property access: store.subscribe(), store.set() → skip
                if after_text.starts_with('.') || after_text.starts_with("?.") { continue; }
                // Plain assignment target: store = writable(...) → skip
                if after_text.starts_with('=') && !after_text.starts_with("==") {
                    // But compound assignment (+=, -=, etc.) is NOT allowed
                    if after_text.starts_with("= ") || after_text.starts_with("=\n") || after_text.starts_with("=\t") {
                        continue;
                    }
                }
                // RHS of assignment: x = store → skip (passing store object)
                // But NOT x = store() — that calls the store's value
                if before.ends_with('=') && !before.ends_with("!=") && !before.ends_with("==")
                    && !before.ends_with("+=") && !before.ends_with("-=")
                    && !after_text.starts_with('(') { continue; }
                // Function argument: fn(store) → skip (but NOT fn(store`...`) — tagged template)
                if (before.ends_with('(') || before.ends_with(", ") || before.ends_with(','))
                    && !after_text.starts_with('`') && !after_text.starts_with('(') { continue; }
                // store() as method DEFINITION (in class) → skip
                // store() as function CALL → flag (raw store access)
                if after_text.starts_with('(') {
                    // Method def: line starts with just the method name (no assignment)
                    let line_trimmed = line.trim_start();
                    let is_method_def = line_trimmed.starts_with(var.as_str())
                        && !line.contains('=') && !line.contains("$:");
                    if is_method_def { continue; }
                }
                // Object value: { x: store } → skip (passing store object)
                // But NOT [store] computed key — that accesses the value
                if before.ends_with(':') && !after_text.starts_with(']') { continue; }
                // Skip: store followed by ) ] , ; → fn argument, array element, statement end
                if after_text.starts_with(')') || after_text.starts_with(']')
                    || after_text.starts_with(',') || after_text.starts_with(';') {
                    continue;
                }

                // FORBIDDEN patterns: store++, --store, -store, +store, ~store, store += x
                let src_pos = content_offset + pos;
                ctx.diagnostic(
                    "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                    oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                );
            }
        }

        // Check template for raw store references (without $ prefix or get())
        let store_vars_clone = store_vars.clone();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            match node {
                TemplateNode::MustacheTag(tag) => {
                    check_expr_for_raw_store(&tag.expression, tag.span, &store_vars_clone, ctx);
                }
                TemplateNode::Element(el) => {
                    // Skip components — passing stores as props is valid
                    let is_component = el.name.chars().next().map_or(false, |c| c.is_uppercase())
                        || el.name.contains('.');
                    if is_component { return; }
                    for attr in &el.attributes {
                        if let Attribute::NormalAttribute { value, span, .. } = attr {
                            match value {
                                AttributeValue::Expression(expr) => {
                                    check_expr_for_raw_store(expr, *span, &store_vars_clone, ctx);
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
                        if let Attribute::Spread { span } = attr {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            for var in &store_vars_clone {
                                if region.contains(var.as_str()) && !region.contains(&format!("${}", var)) {
                                    ctx.diagnostic(
                                        "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
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
