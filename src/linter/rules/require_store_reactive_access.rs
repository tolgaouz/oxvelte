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
