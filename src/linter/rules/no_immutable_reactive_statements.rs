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
        let script = match &ctx.ast.instance { Some(s) => s, None => return };
        let content = &script.content;
        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        // Single pass: classify all declarations and collect reactive statements
        let mut immutable_names: HashSet<&str> = HashSet::new();
        let mut let_names: HashSet<&str> = HashSet::new();
        let mut reactive_stmts: Vec<(usize, &str)> = Vec::new(); // (line_offset, rhs_text)

        for line in content.lines() {
            let trimmed = line.trim();
            let name = extract_decl_name(trimmed);

            if let Some(n) = name {
                if trimmed.starts_with("export let ") {
                    // Props are mutable — skip
                } else if trimmed.starts_with("let ") || trimmed.starts_with("var ") {
                    let_names.insert(n);
                } else if trimmed.starts_with("const ") || trimmed.starts_with("export const ") {
                    // Only primitive const values are truly immutable
                    if is_primitive_init(trimmed) {
                        immutable_names.insert(n);
                    }
                } else if trimmed.starts_with("function ") || trimmed.starts_with("export function ") {
                    immutable_names.insert(n);
                } else if trimmed.starts_with("export class ") || trimmed.starts_with("class ") {
                    immutable_names.insert(n);
                } else if trimmed.starts_with("import ") {
                    // Import names are immutable
                    for imp in extract_import_names(trimmed) {
                        immutable_names.insert(imp);
                    }
                }
            } else if trimmed.starts_with("import ") {
                for imp in extract_import_names(trimmed) {
                    immutable_names.insert(imp);
                }
            }

            // Collect reactive statements
            if trimmed.starts_with("$:") {
                let after = trimmed[2..].trim_start();
                // Get the RHS (after assignment) or the full expression
                let rhs = if let Some(eq) = after.find('=') {
                    let post = &after[eq + 1..];
                    if post.starts_with('=') || post.starts_with('>') { after } else { post }
                } else {
                    after
                };
                if let Some(offset) = content.find(trimmed) {
                    reactive_stmts.push((offset, rhs));
                }
            }
        }

        // Find mutable let vars (reassigned or bound in template)
        let mut mutable_lets: HashSet<&str> = HashSet::new();
        for &var in &let_names {
            // Check for reassignment in content
            if has_reassignment(content, var) {
                mutable_lets.insert(var);
            }
        }

        // Check bind:value in template
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::Binding, span, .. } = attr {
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(open) = region.find('{') {
                            if let Some(close) = region.find('}') {
                                let var = region[open+1..close].trim();
                                if let_names.contains(var) {
                                    mutable_lets.insert(var);
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

        // Const vars used in each blocks are potentially mutable
        for &name in &immutable_names {
            if source.contains(&format!("each {} as", name)) {
                // Can't modify immutable_names while iterating, skip
                // These will be handled by not being in the immutable set
            }
        }
        // Rebuild immutable set excluding each-source consts
        let immutable: HashSet<&str> = immutable_names.iter()
            .filter(|&&n| !source.contains(&format!("each {} as", n)))
            .copied()
            .collect();

        // Add non-reassigned let vars as immutable
        let all_immutable: HashSet<&str> = immutable.iter().copied()
            .chain(let_names.iter().filter(|n| !mutable_lets.contains(*n)).copied())
            .collect();

        // Collect $: declared var names (they are reactive)
        let reactive_var_names: HashSet<&str> = reactive_stmts.iter()
            .filter_map(|(_, _)| None::<&str>) // placeholder
            .collect();
        // Actually extract from content
        let mut reactive_decl_names: HashSet<&str> = HashSet::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("$:") {
                let after = trimmed[2..].trim_start();
                if let Some(eq) = after.find('=') {
                    let name = after[..eq].trim();
                    if name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !name.is_empty() {
                        reactive_decl_names.insert(name);
                    }
                }
            }
        }

        // Check each reactive statement
        for &(offset, rhs) in &reactive_stmts {
            let ids = extract_identifiers(rhs);

            // If references $store or reactive var, it's fine
            if ids.iter().any(|id| id.starts_with('$')) { continue; }
            if ids.iter().any(|id| reactive_decl_names.contains(id.as_str())) { continue; }

            // Filter to declared identifiers only
            let all_declared: HashSet<&str> = all_immutable.iter().copied()
                .chain(mutable_lets.iter().copied())
                .collect();
            let referenced: Vec<&str> = ids.iter()
                .filter(|id| all_declared.contains(id.as_str()))
                .map(|s| s.as_str())
                .collect();

            // Unknown vars (not declared) might be mutable globals
            let has_unknown = ids.iter().any(|id| !all_declared.contains(id.as_str()));
            if has_unknown { continue; }

            // All referenced declared vars are immutable → flag
            if !referenced.is_empty() && referenced.iter().all(|v| all_immutable.contains(v)) {
                let source_pos = content_offset + offset + 2; // skip "$:"
                let end = content_offset + offset + 2 + rhs.len();
                ctx.diagnostic(
                    "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                    oxc::span::Span::new(source_pos as u32, end as u32),
                );
            } else if referenced.is_empty() {
                // No declared vars at all — might be calling immutable functions
                let source_pos = content_offset + offset + 2;
                let trimmed = &content[offset..];
                let line_end = trimmed.find('\n').unwrap_or(trimmed.len());
                let end = content_offset + offset + line_end;
                ctx.diagnostic(
                    "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                    oxc::span::Span::new(source_pos as u32, end as u32),
                );
            }
        }
    }
}

fn extract_decl_name(line: &str) -> Option<&str> {
    let prefixes = ["export const ", "const ", "export let ", "let ", "var ",
                    "export function ", "function ", "export class ", "class "];
    for prefix in &prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                .unwrap_or(rest.len());
            let name = &rest[..end];
            if !name.is_empty() { return Some(name); }
        }
    }
    None
}

fn is_primitive_init(line: &str) -> bool {
    if let Some(eq) = line.find('=') {
        let init = line[eq + 1..].trim().trim_end_matches(';').trim();
        init.starts_with('\'') || init.starts_with('"')
            || (init.starts_with('`') && !init.contains("${"))
            || init.parse::<f64>().is_ok()
            || init == "true" || init == "false" || init == "null" || init == "undefined"
    } else {
        false
    }
}

fn extract_import_names(line: &str) -> Vec<&str> {
    let mut names = Vec::new();
    if let Some(from_pos) = line.find(" from ") {
        let import_part = &line[7..from_pos].trim();
        // Default import
        if !import_part.starts_with('{') && !import_part.starts_with('*') {
            let end = import_part.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                .unwrap_or(import_part.len());
            let name = &import_part[..end];
            if !name.is_empty() { names.push(name); }
        }
        // Named imports
        if let Some(open) = import_part.find('{') {
            if let Some(close) = import_part.find('}') {
                for part in import_part[open+1..close].split(',') {
                    let part = part.trim();
                    let name = if let Some(as_pos) = part.find(" as ") {
                        part[as_pos + 4..].trim()
                    } else { part };
                    if !name.is_empty() { names.push(name); }
                }
            }
        }
    }
    names
}

fn has_reassignment(content: &str, var: &str) -> bool {
    let patterns = [" =", "=", "++", "--", " +=", " -="];
    for suffix in &patterns {
        let search = format!("{}{}", var, suffix);
        for (pos, _) in content.match_indices(&search) {
            if pos > 0 {
                let prev = content.as_bytes()[pos - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' { continue; }
            }
            let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line = content[line_start..].trim_start();
            if line.starts_with("let ") || line.starts_with("var ") || line.starts_with("$:") { continue; }
            if suffix == &" =" || suffix == &"=" {
                let after = pos + search.len();
                if after < content.len() && content.as_bytes()[after] == b'=' { continue; }
            }
            return true;
        }
    }
    false
}

fn extract_identifiers(expr: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let q = bytes[i]; i += 1;
            while i < bytes.len() && bytes[i] != q {
                if bytes[i] == b'\\' { i += 1; }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }
        if bytes[i] == b'`' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' {
                if bytes[i] == b'\\' { i += 2; continue; }
                if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    i += 2;
                    let mut depth = 1;
                    let start = i;
                    while i < bytes.len() && depth > 0 {
                        if bytes[i] == b'{' { depth += 1; }
                        if bytes[i] == b'}' { depth -= 1; }
                        if depth > 0 { i += 1; }
                    }
                    ids.extend(extract_identifiers(&expr[start..i]));
                    if i < bytes.len() { i += 1; }
                    continue;
                }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$' {
            // Skip member access (identifiers after `.`)
            if i > 0 && bytes[i - 1] == b'.' {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                    i += 1;
                }
                continue;
            }
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
            }
            let id = &expr[start..i];
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
