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

        // Classify declarations
        let mut immutable_names: HashSet<&str> = HashSet::new();
        let mut let_names: HashSet<&str> = HashSet::new();

        for line in content.lines() {
            let trimmed = line.trim();
            let name = extract_decl_name(trimmed);

            if let Some(n) = name {
                if trimmed.starts_with("export let ") {
                    // Props are mutable
                } else if trimmed.starts_with("let ") || trimmed.starts_with("var ") {
                    let_names.insert(n);
                } else if trimmed.starts_with("const ") || trimmed.starts_with("export const ") {
                    if is_primitive_init(trimmed) {
                        immutable_names.insert(n);
                    }
                } else if trimmed.starts_with("function ") || trimmed.starts_with("export function ") {
                    immutable_names.insert(n);
                } else if trimmed.starts_with("export class ") || trimmed.starts_with("class ") {
                    immutable_names.insert(n);
                } else if trimmed.starts_with("import ") {
                    for imp in extract_import_names(trimmed) {
                        immutable_names.insert(imp);
                    }
                }
            } else if trimmed.starts_with("import ") {
                for imp in extract_import_names(trimmed) {
                    immutable_names.insert(imp);
                }
            }
        }

        // Collect reactive statements with their FULL text (multiline support)
        let reactive_stmts = collect_reactive_stmts(content);

        // Find mutable let vars (reassigned or bound in template)
        let mut mutable_lets: HashSet<&str> = HashSet::new();
        for &var in &let_names {
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
        let mut reactive_decl_names: HashSet<&str> = HashSet::new();
        for (_, full_text) in &reactive_stmts {
            let after = full_text[2..].trim_start();
            if let Some(eq) = after.find('=') {
                let name = after[..eq].trim();
                if name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !name.is_empty() {
                    reactive_decl_names.insert(name);
                }
            }
        }

        // All declared identifiers
        let all_declared: HashSet<&str> = all_immutable.iter().copied()
            .chain(mutable_lets.iter().copied())
            .collect();

        // Check each reactive statement
        for &(offset, ref full_text) in &reactive_stmts {
            let after = full_text[2..].trim_start();
            // Get the RHS (after assignment) or the full expression.
            // Only treat as assignment if the LHS is a simple identifier
            // (not `if (...)`, `{...}`, function call, etc.)
            let rhs = if let Some(eq) = after.find('=') {
                let lhs = after[..eq].trim();
                let post = &after[eq + 1..];
                if post.starts_with('=') || post.starts_with('>') {
                    // == or => operator, use full expression
                    after
                } else if lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !lhs.is_empty() {
                    // Simple identifier assignment: `$: varName = expr`
                    post
                } else {
                    // Not a simple assignment (e.g., `$: if (...) { ... = ... }`), use full expression
                    after
                }
            } else {
                after
            };

            let ids = extract_identifiers(rhs);

            // If references $store or reactive var, it's fine
            if ids.iter().any(|id| id.starts_with('$')) { continue; }
            if ids.iter().any(|id| reactive_decl_names.contains(id.as_str())) { continue; }

            let referenced: Vec<&str> = ids.iter()
                .filter(|id| all_declared.contains(id.as_str()))
                .map(|s| s.as_str())
                .collect();

            // Unknown vars (not declared in this scope) might be reactive
            let has_unknown = ids.iter().any(|id| !all_declared.contains(id.as_str()));
            if has_unknown { continue; }

            // All referenced declared vars are immutable -> flag
            if !referenced.is_empty() && referenced.iter().all(|v| all_immutable.contains(v)) {
                let source_pos = content_offset + offset;
                let end = content_offset + offset + full_text.len();
                ctx.diagnostic(
                    "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                    oxc::span::Span::new(source_pos as u32, end as u32),
                );
            }
            // If referenced is empty but there ARE identifiers, they're all unknown/global
            // which means we already skipped above. If there are truly NO identifiers at all
            // (e.g., `$: console.log('hello')`), don't flag - it might be intentional side effect.
        }
    }
}

/// Collect reactive statements with multiline support.
/// Returns (byte_offset_in_content, full_statement_text).
fn collect_reactive_stmts(content: &str) -> Vec<(usize, &str)> {
    let mut stmts = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut line_offsets: Vec<usize> = Vec::with_capacity(lines.len());
    let mut off = 0;
    for line in &lines {
        line_offsets.push(off);
        off += line.len() + 1; // +1 for newline
    }

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("$:") {
            let start_offset = line_offsets[i] + (lines[i].len() - lines[i].trim_start().len());
            // Find the end of this statement (may span multiple lines)
            let stmt_end = find_statement_end(content, start_offset);
            let full = &content[start_offset..stmt_end];
            stmts.push((start_offset, full));
        }
        i += 1;
    }
    stmts
}

/// Find the end of a statement starting at `start` in `content`.
/// Handles braces, parens, brackets, and semicolons.
fn find_statement_end(content: &str, start: usize) -> usize {
    let bytes = content.as_bytes();
    let mut i = start;
    // Skip "$:" prefix
    if i + 2 <= bytes.len() && &content[i..i+2] == "$:" {
        i += 2;
    }
    let after = content[i..].trim_start();
    i = content.len() - after.len();

    let mut depth_brace = 0i32;
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' { i += 1; }
                    i += 1;
                }
                if i < bytes.len() { i += 1; }
                continue;
            }
            b'`' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'`' {
                    if bytes[i] == b'\\' { i += 1; }
                    else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                        i += 2;
                        let mut d = 1;
                        while i < bytes.len() && d > 0 {
                            if bytes[i] == b'{' { d += 1; }
                            if bytes[i] == b'}' { d -= 1; }
                            if d > 0 { i += 1; }
                        }
                    }
                    i += 1;
                }
                if i < bytes.len() { i += 1; }
                continue;
            }
            b'{' => depth_brace += 1,
            b'}' => {
                depth_brace -= 1;
                if depth_brace < 0 { return i; }
                // After closing a top-level block, the statement is done
                if depth_brace == 0 && depth_paren == 0 && depth_bracket == 0 {
                    i += 1;
                    // Skip trailing whitespace/semicolons on same line
                    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
                    return i;
                }
            }
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b';' if depth_brace == 0 && depth_paren == 0 && depth_bracket == 0 => {
                return i + 1;
            }
            b'\n' if depth_brace == 0 && depth_paren == 0 && depth_bracket == 0 => {
                // Statement ends at newline if all brackets are balanced,
                // UNLESS the current line ends with an operator or the next
                // non-whitespace line starts with one (continuation).
                let before_nl = content[start..i].trim_end();
                let after_nl = content[i + 1..].trim_start();
                let continues = before_nl.ends_with('?') || before_nl.ends_with(':')
                    || before_nl.ends_with("||") || before_nl.ends_with("&&")
                    || before_nl.ends_with('+') || before_nl.ends_with('-')
                    || before_nl.ends_with(',') || before_nl.ends_with('\\')
                    || before_nl.ends_with("??")
                    || after_nl.starts_with('?') || after_nl.starts_with(':')
                    || after_nl.starts_with("||") || after_nl.starts_with("&&")
                    || after_nl.starts_with("??")
                    || after_nl.starts_with('.') || after_nl.starts_with("?.");
                if continues {
                    i += 1;
                    continue;
                }
                return i;
            }
            _ => {}
        }
        i += 1;
    }
    content.len()
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
