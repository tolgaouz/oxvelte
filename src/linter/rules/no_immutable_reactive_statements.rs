//! `svelte/no-immutable-reactive-statements` — disallow reactive statements that don't reference reactive values.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
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
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

            // Step 1: Find all declared variables and classify as mutable/immutable
            let mut let_vars: HashSet<String> = HashSet::new();
            let mut const_vars: HashSet<String> = HashSet::new();
            let mut import_vars: HashSet<String> = HashSet::new();
            let mut mutable_lets: HashSet<String> = HashSet::new();

            for line in content.lines() {
                let trimmed = line.trim();
                // const declarations
                if trimmed.starts_with("const ") || trimmed.starts_with("export const ") {
                    let rest = if trimmed.starts_with("export ") { &trimmed[7..] } else { trimmed };
                    let rest = rest.strip_prefix("const ").unwrap_or(rest);
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if !name.is_empty() {
                        const_vars.insert(name.to_string());
                    }
                }
                // export let (props are always mutable)
                else if trimmed.starts_with("export let ") {
                    let rest = &trimmed[11..];
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if !name.is_empty() {
                        let_vars.insert(name.to_string());
                        mutable_lets.insert(name.to_string()); // props are always mutable
                    }
                }
                // let/var declarations
                else if trimmed.starts_with("let ") || trimmed.starts_with("var ") {
                    let kw_len = if trimmed.starts_with("let ") { 4 } else { 4 };
                    let rest = &trimmed[kw_len..];
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if !name.is_empty() {
                        let_vars.insert(name.to_string());
                    }
                }
                // import declarations
                else if trimmed.starts_with("import ") {
                    // Extract imported names (simplified)
                    if let Some(from_pos) = trimmed.find(" from ") {
                        let import_part = &trimmed[7..from_pos];
                        // Default import: import name from '...'
                        let import_part = import_part.trim();
                        if !import_part.starts_with('{') && !import_part.starts_with('*') {
                            let name_end = import_part.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                                .unwrap_or(import_part.len());
                            let name = &import_part[..name_end];
                            if !name.is_empty() {
                                import_vars.insert(name.to_string());
                            }
                        }
                        // Named imports: import { a, b } from '...'
                        if let Some(open) = import_part.find('{') {
                            if let Some(close) = import_part.find('}') {
                                let names = &import_part[open+1..close];
                                for name in names.split(',') {
                                    let name = name.trim();
                                    // Handle "original as alias"
                                    let name = if let Some(as_pos) = name.find(" as ") {
                                        name[as_pos + 4..].trim()
                                    } else { name };
                                    if !name.is_empty() {
                                        import_vars.insert(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                // export function / export class
                else if trimmed.starts_with("export function ") {
                    let rest = &trimmed[16..];
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if !name.is_empty() {
                        const_vars.insert(name.to_string());
                    }
                }
                else if trimmed.starts_with("export class ") {
                    let rest = &trimmed[13..];
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if !name.is_empty() {
                        const_vars.insert(name.to_string());
                    }
                }
                // function declarations at top level
                else if trimmed.starts_with("function ") {
                    let rest = &trimmed[9..];
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if !name.is_empty() {
                        const_vars.insert(name.to_string());
                    }
                }
            }

            // Step 2: Find mutable let vars (reassigned or used with bind:)
            for var in &let_vars {
                // Check for reassignment patterns: var = , var++, var--, var +=, etc.
                let assign_patterns = [
                    format!("{} =", var), format!("{}=", var),
                    format!("{}++", var), format!("{}--", var),
                    format!("{} +=", var), format!("{} -=", var),
                ];
                for pattern in &assign_patterns {
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(pattern.as_str()) {
                        let abs = search_from + pos;
                        // Check word boundary before
                        if abs > 0 {
                            let prev = content.as_bytes()[abs - 1];
                            if prev.is_ascii_alphanumeric() || prev == b'_' {
                                search_from = abs + 1;
                                continue;
                            }
                        }
                        // Skip the declaration line itself
                        let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = content[line_start..].trim_start();
                        if line.starts_with("let ") || line.starts_with("var ") {
                            search_from = abs + 1;
                            continue;
                        }
                        // Skip == comparison
                        if pattern.ends_with(" =") || pattern.ends_with('=') {
                            let after = abs + pattern.len();
                            if after < content.len() && content.as_bytes()[after] == b'=' {
                                search_from = abs + 1;
                                continue;
                            }
                        }
                        mutable_lets.insert(var.clone());
                        break;
                    }
                    if mutable_lets.contains(var) { break; }
                }
            }

            // Check for bind:value={var} in template
            walk_template_nodes(&ctx.ast.html, &mut |node| {
                if let TemplateNode::Element(el) = node {
                    for attr in &el.attributes {
                        if let Attribute::Directive { kind: DirectiveKind::Binding, span, .. } = attr {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            if let Some(open) = region.find('{') {
                                if let Some(close) = region.find('}') {
                                    let var = region[open+1..close].trim();
                                    if let_vars.contains(var) {
                                        mutable_lets.insert(var.to_string());
                                    }
                                }
                            }
                            // Shorthand bind:value (var name = attr name)
                            if !region.contains('{') && !region.contains('=') {
                                // bind:value → var is "value"
                                if let Some(colon_pos) = region.find(':') {
                                    let name = region[colon_pos+1..].trim();
                                    if let_vars.contains(name) {
                                        mutable_lets.insert(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            });

            // Separate const into truly immutable (functions, classes) vs potentially mutable (values)
            let mut immutable_consts: HashSet<String> = HashSet::new();
            for line in content.lines() {
                let trimmed = line.trim();
                // Functions and classes are truly immutable
                if trimmed.starts_with("function ") || trimmed.starts_with("export function ") {
                    // Already in const_vars
                }
                if trimmed.starts_with("export class ") || trimmed.starts_with("class ") {
                    // Already in const_vars
                }
            }
            // Only function/class declarations are truly immutable consts
            // Value consts (arrays, objects, primitives) might be mutated via member access
            // So we only mark function/class names as immutable
            for line in content.lines() {
                let trimmed = line.trim();
                for prefix in &["function ", "export function "] {
                    if let Some(rest) = trimmed.strip_prefix(prefix) {
                        let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                            .unwrap_or(rest.len());
                        let name = &rest[..name_end];
                        if !name.is_empty() { immutable_consts.insert(name.to_string()); }
                    }
                }
                for prefix in &["export class ", "class "] {
                    if let Some(rest) = trimmed.strip_prefix(prefix) {
                        let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                            .unwrap_or(rest.len());
                        let name = &rest[..name_end];
                        if !name.is_empty() { immutable_consts.insert(name.to_string()); }
                    }
                }
                // const assigned to a primitive literal (string, number, boolean)
                if let Some(rest) = trimmed.strip_prefix("const ").or_else(|| trimmed.strip_prefix("export const ")) {
                    let rest = if trimmed.starts_with("export ") { &trimmed[7..] } else { trimmed };
                    let rest = rest.strip_prefix("const ").unwrap_or(rest);
                    let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    let name = &rest[..name_end];
                    if let Some(eq_pos) = rest.find('=') {
                        let init = rest[eq_pos + 1..].trim().trim_end_matches(';').trim();
                        // Only truly immutable if initialized to a primitive
                        let is_primitive = init.starts_with('\'') || init.starts_with('"')
                            || init.starts_with('`') && !init.contains("${")
                            || init.parse::<f64>().is_ok()
                            || init == "true" || init == "false" || init == "null" || init == "undefined";
                        if is_primitive && !name.is_empty() {
                            immutable_consts.insert(name.to_string());
                        }
                    }
                }
            }

            // Build the set of immutable variables
            let mut immutable: HashSet<&str> = HashSet::new();
            for v in &immutable_consts { immutable.insert(v.as_str()); }
            for v in &import_vars { immutable.insert(v.as_str()); }
            for v in &let_vars {
                if !mutable_lets.contains(v) {
                    immutable.insert(v.as_str());
                }
            }

            // Collect $: declared variables (they are always mutable/reactive)
            let mut reactive_vars: HashSet<String> = HashSet::new();
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("$:") {
                    let after = trimmed[2..].trim_start();
                    if let Some(eq_pos) = after.find('=') {
                        let name = after[..eq_pos].trim();
                        if name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            && !name.is_empty()
                        {
                            reactive_vars.insert(name.to_string());
                        }
                    }
                }
            }

            // Step 3: Check each $: statement
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.starts_with("$:") { continue; }
                let after_label = &trimmed[2..].trim_start();

                // For assignments ($: x = expr), only check the RHS
                let check_part = if let Some(eq_pos) = after_label.find('=') {
                    // Make sure it's not == or =>
                    let after_eq = &after_label[eq_pos + 1..];
                    if after_eq.starts_with('=') || after_eq.starts_with('>') {
                        *after_label
                    } else {
                        after_eq
                    }
                } else {
                    *after_label
                };

                // Extract identifiers referenced in this statement
                let referenced = extract_identifiers(check_part);

                // Check for $store references (always reactive)
                let has_store_ref = referenced.iter().any(|id| id.starts_with('$'));

                // Check for reactive var references
                let has_reactive_ref = referenced.iter().any(|id| reactive_vars.contains(id.as_str()));

                if has_store_ref || has_reactive_ref {
                    continue; // This statement references reactive values
                }

                // Filter to only declared variables (not built-ins like console, etc.)
                let all_declared: HashSet<&str> = const_vars.iter().map(|s| s.as_str())
                    .chain(import_vars.iter().map(|s| s.as_str()))
                    .chain(let_vars.iter().map(|s| s.as_str()))
                    .collect();

                let referenced_declared: Vec<&str> = referenced.iter()
                    .filter(|id| all_declared.contains(id.as_str()))
                    .map(|s| s.as_str())
                    .collect();

                // If all referenced declared variables are immutable, flag it
                if !referenced_declared.is_empty()
                    && referenced_declared.iter().all(|v| immutable.contains(v))
                {
                    // Find position in source
                    if let Some(pos) = content.find(trimmed) {
                        let source_pos = content_offset + pos;
                        // Find position after "$: " for the diagnostic
                        let diag_start = source_pos + 2; // skip "$:"
                        let stmt_end = source_pos + trimmed.len();
                        ctx.diagnostic(
                            "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                            oxc::span::Span::new(diag_start as u32, stmt_end as u32),
                        );
                    }
                }
                // If NO declared variables referenced, check if all identifiers are known immutable
                else if referenced_declared.is_empty() && !after_label.is_empty() {
                    // If there are unknown identifiers (not declared), they might be mutable globals
                    let has_unknown = referenced.iter().any(|id| !all_declared.contains(id.as_str()));
                    if has_unknown {
                        continue; // Unknown vars might be mutable, don't flag
                    }
                    // Only function calls to immutable functions
                    if let Some(pos) = content.find(trimmed) {
                        let source_pos = content_offset + pos;
                        let diag_start = source_pos + 2;
                        let stmt_end = source_pos + trimmed.len();
                        ctx.diagnostic(
                            "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                            oxc::span::Span::new(diag_start as u32, stmt_end as u32),
                        );
                    }
                }
            }
        }
    }
}

/// Extract simple identifiers from a JS expression (heuristic).
fn extract_identifiers(expr: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip single/double quoted strings (but NOT template literals — they contain expressions)
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let q = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != q {
                if bytes[i] == b'\\' { i += 1; }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }
        // For template literals, skip the static parts but extract identifiers from ${...}
        if bytes[i] == b'`' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' {
                if bytes[i] == b'\\' { i += 2; continue; }
                if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    i += 2; // skip ${
                    let mut depth = 1;
                    let expr_start = i;
                    while i < bytes.len() && depth > 0 {
                        if bytes[i] == b'{' { depth += 1; }
                        if bytes[i] == b'}' { depth -= 1; }
                        if depth > 0 { i += 1; }
                    }
                    // Extract identifiers from the interpolation
                    let inner = &expr[expr_start..i];
                    ids.extend(extract_identifiers(inner));
                    if i < bytes.len() { i += 1; } // skip }
                    continue;
                }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }
        // Collect identifiers
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
            }
            let id = &expr[start..i];
            // Skip JS keywords
            if !matches!(id, "true" | "false" | "null" | "undefined" | "new" | "typeof"
                | "if" | "else" | "return" | "const" | "let" | "var" | "function"
                | "class" | "this" | "console" | "Math" | "JSON" | "Object" | "Array"
                | "String" | "Number" | "Boolean" | "Date" | "Error" | "Promise"
                | "Map" | "Set" | "RegExp" | "Symbol" | "BigInt" | "Infinity" | "NaN") {
                ids.push(id.to_string());
            }
            continue;
        }
        i += 1;
    }
    ids
}
