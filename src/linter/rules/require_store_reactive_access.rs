//! `svelte/require-store-reactive-access` — require `$store` syntax for reactive access.
//! ⭐ Recommended 🔧 Fixable

use crate::linter::{parse_imports, walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, AttributeValuePart};
use std::collections::{HashSet, HashMap};

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
        // Track whether each store is declared with const (is_const=true) or let (is_const=false)
        let mut store_vars_map: HashMap<String, bool> = HashMap::new();
        for line in content.lines() {
            let trimmed = line.trim();
            for (prefix, is_const) in &[("const ", true), ("let ", false)] {
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
                            store_vars_map.insert(name.to_string(), *is_const);
                        }
                    }
                }
            }
        }

        if store_vars_map.is_empty() { return; }
        let store_vars: HashSet<String> = store_vars_map.keys().cloned().collect();
        // const-only stores (for "consistent" mode used by class directives, if blocks, etc.)
        let const_store_vars: HashSet<String> = store_vars_map.iter()
            .filter(|(_, is_const)| **is_const)
            .map(|(name, _)| name.clone())
            .collect();

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
        // Based on vendor eslint-plugin-svelte implementation.
        // "consistent" mode = only flag const stores (let stores can be reassigned).
        // Consistent contexts: typeof, !, ==, !=, ===, !==, &&, ||, ??, ternary, await, if()
        // Non-consistent (flag all): ++/--, -/+/~, arithmetic, compound assignment, import(),
        //   switch(), for...in/of, tagged templates, function calls on store
        let is_const_store = |v: &str| store_vars_map.get(v).copied() == Some(true);
        for var in &store_vars {
            for (pos, _) in content.match_indices(var.as_str()) {
                // Word boundary
                if pos > 0 {
                    let p = content.as_bytes()[pos - 1];
                    if p.is_ascii_alphanumeric() || p == b'_' || p == b'$' { continue; }
                    // Skip property access (obj.store) but NOT spread (...store)
                    if p == b'.' {
                        let is_spread = pos >= 3 && &content[pos-3..pos] == "...";
                        if !is_spread { continue; }
                    }
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
                // Skip eslint-disable comments
                if line.starts_with("// eslint-") { continue; }
                // Skip get() call
                let before = content[..pos].trim_end();
                if before.ends_with("get(") { continue; }
                // Skip if $var on same line
                let line_end = content[pos..].find('\n').map(|p| pos + p).unwrap_or(content.len());
                let full_line = &content[line_start..line_end];
                if full_line.contains(&format!("${}", var)) { continue; }

                let after_text = if after < content.len() { content[after..].trim_start() } else { "" };

                // Property access: store.subscribe(), store.set() → always skip
                if after_text.starts_with('.') || after_text.starts_with("?.") { continue; }

                // Plain assignment target: store = expr → skip (reassigning the store variable)
                if after_text.starts_with('=') && !after_text.starts_with("==") {
                    // But compound assignment (+=, -=, etc.) is NOT allowed
                    if after_text.starts_with("= ") || after_text.starts_with("=\n") || after_text.starts_with("=\t") {
                        continue;
                    }
                }

                // RHS of assignment: x = store → skip (passing store object)
                // But NOT compound assignment (x += store) → flag
                if before.ends_with('=') && !before.ends_with("!=") && !before.ends_with("==")
                    && !before.ends_with("+=") && !before.ends_with("-=")
                    && !before.ends_with("*=") && !before.ends_with("/=")
                    && !after_text.starts_with('(') { continue; }

                // Object value: { x: store } → skip (passing store object)
                if before.ends_with(':') && !after_text.starts_with(']') { continue; }

                // --- CONSISTENT MODE contexts (only flag const stores) ---
                let consistent_context =
                    // typeof store, !store
                    before.ends_with("typeof") || before.ends_with("typeof ")
                    || before.ends_with('!')
                    // store == x, store != x, store === x, store !== x
                    || after_text.starts_with("==") || after_text.starts_with("!=")
                    // store && x, store || x, store ?? x
                    || after_text.starts_with("&&") || after_text.starts_with("||") || after_text.starts_with("??")
                    // store ? x : y (ternary)
                    || (after_text.starts_with('?') && !after_text.starts_with("?."))
                    // await store
                    || before.ends_with("await") || before.ends_with("await ");

                if consistent_context {
                    if is_const_store(var) {
                        let src_pos = content_offset + pos;
                        ctx.diagnostic(
                            "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                            oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                        );
                    }
                    continue;
                }

                // --- Detect control flow keywords before ( ---
                // if (store) → consistent mode (only flag const)
                // switch(store), while(store) → flag all
                // for (... in store), for (... of store) → flag all
                let before_trimmed = before.trim_end();
                let in_for_in_of = before_trimmed.ends_with(" in") || before_trimmed.ends_with(" of")
                    || before_trimmed.ends_with("\tin") || before_trimmed.ends_with("\tof");

                if in_for_in_of {
                    // for...in/of → flag all stores, don't skip
                    let src_pos = content_offset + pos;
                    ctx.diagnostic(
                        "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                        oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                    );
                    continue;
                }

                // Check if inside control flow parens: if(store), switch(store)
                if before.ends_with('(') {
                    let kw_before = before[..before.len()-1].trim_end();
                    if kw_before.ends_with("if") {
                        // if (store) → consistent mode
                        if is_const_store(var) {
                            let src_pos = content_offset + pos;
                            ctx.diagnostic(
                                "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                                oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                            );
                        }
                        continue;
                    }
                    if kw_before.ends_with("switch") || kw_before.ends_with("while") {
                        // switch/while → flag all
                        let src_pos = content_offset + pos;
                        ctx.diagnostic(
                            "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                            oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                        );
                        continue;
                    }
                }

                // Function argument: fn(store) → skip (passing store object)
                // Exceptions: import(store), tagged templates store`...`
                if (before.ends_with('(') || before.ends_with(", ") || before.ends_with(','))
                    && !after_text.starts_with('`') && !after_text.starts_with('(')
                    && !before.ends_with("import(") { continue; }

                // store() as function CALL → flag (raw store access)
                if after_text.starts_with('(') {
                    let line_trimmed = line.trim_start();
                    let is_method_def = line_trimmed.starts_with(var.as_str())
                        && !line.contains('=') && !line.contains("$:");
                    if is_method_def { continue; }
                }

                // Skip: store followed by ) , ; → fn argument, statement end
                // But NOT when inside import(), or preceded by ... (spread reads value)
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

                // FORBIDDEN patterns: store++, --store, -store, +store, ~store, store += x, store`tag`
                let src_pos = content_offset + pos;
                ctx.diagnostic(
                    "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                    oxc::span::Span::new(src_pos as u32, (src_pos + var.len()) as u32),
                );
            }
        }

        // Check template for raw store references (without $ prefix or get())
        let store_vars_clone = store_vars.clone();
        let const_store_vars_clone = const_store_vars.clone();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            match node {
                TemplateNode::MustacheTag(tag) => {
                    check_expr_for_raw_store(&tag.expression, tag.span, &store_vars_clone, ctx);
                }
                TemplateNode::IfBlock(block) => {
                    // IfBlock uses consistent mode — only flag const stores
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
                                    // For components, skip normal prop={store} (passing store object)
                                    // But flag CSS custom properties (--var={store})
                                    if !is_component || is_css_var {
                                        check_expr_for_raw_store(expr, *span, &store_vars_clone, ctx);
                                    }
                                }
                                AttributeValue::Concat(parts) => {
                                    // Always check concat (interpolation reads the value)
                                    for part in parts {
                                        if let AttributeValuePart::Expression(expr) = part {
                                            check_expr_for_raw_store(expr, *span, &store_vars_clone, ctx);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        // Check directive values for raw store references
                        if let Attribute::Directive { kind, name: dir_name, span, .. } = attr {
                            // For components: bind:value={store} is OK (passing store),
                            // but bind:this={store} is flagged (needs reactive)
                            if is_component {
                                let is_bind_this = matches!(kind, crate::ast::DirectiveKind::Binding)
                                    && dir_name == "this";
                                if !is_bind_this { continue; }
                            }
                            // Class directives use "consistent" mode: only flag const stores
                            let check_vars = if matches!(kind, crate::ast::DirectiveKind::Class) {
                                &const_store_vars_clone
                            } else {
                                &store_vars_clone
                            };
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            let is_shorthand = !region.contains('=');
                            for var in check_vars {
                                // Shorthand: use:store, style:color, class:store — name IS the store
                                if dir_name == var.as_str() && is_shorthand {
                                    ctx.diagnostic(
                                        "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
                                        *span,
                                    );
                                    continue;
                                }
                                // Value: style:color={store}, on:click={handleClick}, class:name={constStore}
                                if let Some(eq) = region.find('=') {
                                    let val = &region[eq+1..];
                                    if let Some(open) = val.find('{') {
                                        if let Some(close) = val.find('}') {
                                            let expr = val[open+1..close].trim();
                                            if expr == var.as_str() && !expr.starts_with('$') {
                                                ctx.diagnostic(
                                                    "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
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
