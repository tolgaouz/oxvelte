//! `svelte/no-immutable-reactive-statements` — disallow reactive statements that don't reference reactive values.
//! ⭐ Recommended

use std::collections::HashMap;
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
        let mut const_names: HashSet<&str> = HashSet::new();
        let mut let_names: HashSet<&str> = HashSet::new();
        let mut prop_names: HashSet<&str> = HashSet::new();

        for line in content.lines() {
            let trimmed = line.trim();
            let name = extract_decl_name(trimmed);

            if let Some(n) = name {
                if trimmed.starts_with("export let ") {
                    // Props are mutable
                    prop_names.insert(n);
                } else if trimmed.starts_with("let ") || trimmed.starts_with("var ") {
                    let_names.insert(n);
                } else if trimmed.starts_with("const ") || trimmed.starts_with("export const ") {
                    // const vars are immutable unless their members are written.
                    // Skip names shadowing props or lets (different scopes).
                    if !prop_names.contains(n) && !let_names.contains(n) {
                        const_names.insert(n);
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

        // Also extract imports from multi-line import statements
        // (the line-by-line approach above misses these)
        extract_multiline_imports(content, &mut immutable_names);

        // Collect reactive statements with their FULL text (multiline support)
        let reactive_stmts = collect_reactive_stmts(content);

        // Find mutable let vars (reassigned or bound in template)
        let mut mutable_lets: HashSet<&str> = HashSet::new();
        for &var in &let_names {
            // Check both script content and full source (template event handlers)
            if has_reassignment(content, var) || has_reassignment(ctx.source, var) {
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

        // Classify const variables: immutable unless they have member writes
        // or are bound in template directives
        let mut const_member_written: HashSet<&str> = HashSet::new();

        // Check for member writes in script and template text
        for &var in &const_names {
            if has_member_write(content, var) || has_member_write(ctx.source, var) {
                const_member_written.insert(var);
            }
        }

        // Check if const vars are bound via bind: directives (implicit member write)
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::Binding, span, .. } = attr {
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(open) = region.find('{') {
                            if let Some(close) = region.find('}') {
                                let expr = region[open+1..close].trim();
                                // Check if the expression references a const var member
                                for &var in &const_names {
                                    if expr.starts_with(var) && expr.len() > var.len() {
                                        let next = expr.as_bytes()[var.len()];
                                        if next == b'.' || next == b'[' {
                                            const_member_written.insert(var);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        for &var in &const_names {
            if !const_member_written.contains(var) {
                immutable_names.insert(var);
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

        // All declared identifiers (including props, which are mutable)
        let all_declared: HashSet<&str> = all_immutable.iter().copied()
            .chain(mutable_lets.iter().copied())
            .chain(prop_names.iter().copied())
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

            let mut ids = extract_identifiers(rhs);

            // For non-simple-assignment reactive blocks (like $: if (cond) varName = expr),
            // remove write-ONLY identifiers (assignment targets that don't also appear as reads).
            // The vendor only considers READ references for immutability checking.
            if rhs == after {
                let write_targets = extract_assignment_targets(rhs);
                if !write_targets.is_empty() {
                    // Count occurrences of each identifier. If it appears more times than
                    // it's a write target, it's also read → keep it.
                    let mut write_counts: HashMap<&str, usize> = HashMap::new();
                    for t in &write_targets {
                        *write_counts.entry(t).or_insert(0) += 1;
                    }
                    let mut id_counts: HashMap<String, usize> = HashMap::new();
                    for id in &ids {
                        *id_counts.entry(id.clone()).or_insert(0) += 1;
                    }
                    // Only remove if ALL occurrences are write-only
                    ids.retain(|id| {
                        let total = id_counts.get(id).copied().unwrap_or(0);
                        let writes = write_counts.get(id.as_str()).copied().unwrap_or(0);
                        total > writes // keep if there are more total refs than writes
                    });
                }
            }

            // If references $store or reactive var, it's fine
            if ids.iter().any(|id| id.starts_with('$')) { continue; }
            if ids.iter().any(|id| reactive_decl_names.contains(id.as_str())) { continue; }

            let referenced: Vec<&str> = ids.iter()
                .filter(|id| all_declared.contains(id.as_str()))
                .map(|s| s.as_str())
                .collect();

            // Collect locally-scoped names within the reactive statement
            // (arrow params, function params, local const/let/var declarations).
            let local_names = collect_local_names(rhs);

            // Unknown vars (not declared in this scope) might be reactive
            let has_unknown = ids.iter().any(|id| {
                !all_declared.contains(id.as_str()) && !local_names.contains(id.as_str())
            });
            if has_unknown { continue; }

            // All referenced declared vars are immutable -> flag
            let should_flag = if !referenced.is_empty() {
                referenced.iter().all(|v| all_immutable.contains(v))
            } else if ids.is_empty() && rhs != after {
                // Simple assignment with literal RHS (e.g., `$: x = false;`)
                // No reactive references at all → statement is not reactive
                let trimmed_rhs = rhs.trim().trim_end_matches(';').trim();
                is_literal_value(trimmed_rhs)
            } else {
                false
            };
            if should_flag {
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

fn is_literal_value(s: &str) -> bool {
    s == "true" || s == "false" || s == "null" || s == "undefined"
        || s.parse::<f64>().is_ok()
        || (s.starts_with('\'') && s.ends_with('\''))
        || (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('`') && s.ends_with('`') && !s.contains("${"))
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

/// Extract import names from multi-line import statements in the full content.
fn extract_multiline_imports<'a>(content: &'a str, immutable_names: &mut HashSet<&'a str>) {
    let mut i = 0;
    let bytes = content.as_bytes();
    while i < bytes.len() {
        // Find "import" at a line start (with optional whitespace)
        if i > 0 && bytes[i - 1] != b'\n' { i += 1; continue; }
        let line_start = i;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() && bytes[i] != b'\n' { i += 1; }
        if !bytes[i].is_ascii_alphabetic() { i = line_start + 1; continue; }
        if i + 6 < bytes.len() && bytes[i] == b'i' && bytes[i+1] == b'm' && bytes[i+2] == b'p'
            && bytes[i+3] == b'o' && bytes[i+4] == b'r' && bytes[i+5] == b't' {
            // Find the end of the import statement (the line with "from")
            let import_start = i;
            // Scan forward until we find "from " on a line
            let mut end = i;
            while end < bytes.len() {
                if bytes[end] == b'\n' {
                    let next_line = &content[end + 1..];
                    let trimmed = next_line.trim_start();
                    if trimmed.starts_with("from ") || trimmed.starts_with("} from ") {
                        // Find end of this line
                        let nl = next_line.find('\n').unwrap_or(next_line.len());
                        end = end + 1 + nl;
                        break;
                    }
                    // Also check if current accumulated text has "from"
                    if content[import_start..end].contains(" from ") {
                        break;
                    }
                }
                end += 1;
            }
            let full_import = &content[import_start..end];
            // Extract names from the full import using the existing function
            // First, collapse to single line
            let collapsed: String = full_import.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
            for name in extract_import_names(&collapsed) {
                // Find the name in the original content for correct lifetime
                if let Some(pos) = content[import_start..end].find(name) {
                    let actual = &content[import_start + pos..import_start + pos + name.len()];
                    immutable_names.insert(actual);
                }
            }
            i = end;
        } else {
            i = line_start + 1;
        }
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

    // Also check destructuring assignments: `[var] = expr` or `{ x: var } = expr`
    let destructure_patterns = [
        format!("[{}]", var),    // [var] = ...
        format!(", {}]", var),   // [a, var] = ...
        format!("[{},", var),    // [var, b] = ...
        format!(": {} }}", var), // { x: var } = ...
        format!(": {}}}", var),  // {x:var} = ...
    ];
    for pat in &destructure_patterns {
        for (pos, _) in content.match_indices(pat.as_str()) {
            // Check that after the pattern (skipping whitespace), there's `=` (not `==`)
            let after_pat = pos + pat.len();
            let rest = content[after_pat..].trim_start();
            if rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>") {
                let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line = content[line_start..].trim_start();
                if !line.starts_with("let ") && !line.starts_with("var ") && !line.starts_with("const ") && !line.starts_with("$:") {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if a variable has member write access (e.g., `var.prop = ...`, `var[idx] = ...`)
fn has_member_write(content: &str, var: &str) -> bool {
    // Check patterns like: var.something = or var[something] =
    let dot_pat = format!("{}.", var);
    let bracket_pat = format!("{}[", var);

    for pat in &[&dot_pat, &bracket_pat] {
        let is_bracket = pat == &&bracket_pat;
        for (pos, _) in content.match_indices(pat.as_str()) {
            // Verify word boundary before var
            if pos > 0 {
                let prev = content.as_bytes()[pos - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' { continue; }
            }
            // Find the end of the member expression and check for assignment
            let after_var = pos + pat.len();
            let rest = &content[after_var..];
            let mut i = 0;
            let bytes = rest.as_bytes();

            // If pattern was `var[`, skip the bracket expression first
            if is_bracket {
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'[' { depth += 1; }
                    if bytes[i] == b']' { depth -= 1; }
                    i += 1;
                }
            } else {
                // For `var.`, skip the property name
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            }

            // Skip further member chain (e.g., var.a.b.c = ...)
            while i < bytes.len() {
                match bytes[i] {
                    b'.' => {
                        i += 1;
                        // skip identifier
                        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
                    }
                    b'[' => {
                        // skip bracket expression
                        let mut depth = 1;
                        i += 1;
                        while i < bytes.len() && depth > 0 {
                            if bytes[i] == b'[' { depth += 1; }
                            if bytes[i] == b']' { depth -= 1; }
                            i += 1;
                        }
                    }
                    _ => break,
                }
            }
            // Check what follows the member expression
            while i < bytes.len() && bytes[i].is_ascii_whitespace() { i += 1; }
            if i < bytes.len() {
                match bytes[i] {
                    b'=' if i + 1 < bytes.len() && bytes[i + 1] != b'=' => return true,
                    b'+' | b'-' if i + 1 < bytes.len() && bytes[i + 1] == bytes[i] => return true, // ++ or --
                    b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'
                        if i + 1 < bytes.len() && bytes[i + 1] == b'=' => return true, // +=, -=, etc.
                    _ => {}
                }
            }
        }
    }
    false
}

/// Extract simple identifier names that are assignment targets (LHS of `=`) in the expression.
/// Only returns identifiers that are directly assigned (not member expressions).
fn extract_assignment_targets(expr: &str) -> HashSet<&str> {
    let mut targets = HashSet::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    let mut str_ch = 0u8;
    let mut depth = 0i32;

    while i < bytes.len() {
        if in_str {
            if bytes[i] == b'\\' { i += 2; continue; }
            if bytes[i] == str_ch { in_str = false; }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'\'' | b'"' | b'`' => { in_str = true; str_ch = bytes[i]; i += 1; }
            b'{' | b'(' | b'[' => { depth += 1; i += 1; }
            b'}' | b')' | b']' => { depth -= 1; i += 1; }
            b'=' if i + 1 < bytes.len() && bytes[i + 1] != b'=' && bytes[i + 1] != b'>' => {
                // Found assignment `=` (not `==` or `=>`)
                // Look backwards past whitespace to find an identifier
                let mut j = i;
                while j > 0 && bytes[j - 1].is_ascii_whitespace() { j -= 1; }
                let end = j;
                while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_' || bytes[j - 1] == b'$') {
                    j -= 1;
                }
                if j < end {
                    // Check it's a simple identifier (not preceded by `.` which would make it a member expr)
                    if j == 0 || (bytes[j - 1] != b'.' && !bytes[j - 1].is_ascii_alphanumeric() && bytes[j - 1] != b'_') {
                        targets.insert(&expr[j..end]);
                    }
                }
                i += 1;
            }
            _ => { i += 1; }
        }
    }
    targets
}

/// Collect identifiers that are locally declared within an expression:
/// arrow function parameters `(x, y) =>`, function parameters `function(x)`,
/// local `const/let/var` declarations inside blocks.
fn collect_local_names(expr: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let bytes = expr.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip strings
        if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
            let q = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' { i += 2; continue; }
                if bytes[i] == q { break; }
                if q == b'`' && bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
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

        // Look for arrow function: `(params) =>` or `param =>`
        if bytes[i] == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            // Look backwards to find the parameters
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_whitespace() { j -= 1; }
            if j > 0 && bytes[j - 1] == b')' {
                // Find matching `(`
                let mut depth = 1;
                let mut k = j - 2;
                while depth > 0 {
                    if bytes[k] == b')' { depth += 1; }
                    if bytes[k] == b'(' { depth -= 1; }
                    if depth > 0 { if k == 0 { break; } k -= 1; }
                }
                // Extract param names from (params)
                let params = &expr[k + 1..j - 1];
                extract_param_names(params, &mut names);
            } else {
                // Single param: `x =>`
                let end = j;
                while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_' || bytes[j - 1] == b'$') {
                    j -= 1;
                }
                if j < end {
                    names.insert(expr[j..end].to_string());
                }
            }
            i += 2;
            continue;
        }

        // Look for `function(params)` or `function name(params)`
        if i + 8 < bytes.len() && &expr[i..i + 8] == "function" {
            let after = i + 8;
            let rest = expr[after..].trim_start();
            let offset = expr.len() - rest.len();
            if let Some(open) = rest.find('(') {
                if let Some(close) = rest[open..].find(')') {
                    let params = &rest[open + 1..open + close];
                    extract_param_names(params, &mut names);
                    i = offset + open + close + 1;
                    continue;
                }
            }
        }

        // Look for local declarations: `const x`, `let x`, `var x`
        for kw in &["const ", "let ", "var "] {
            if i + kw.len() <= bytes.len() && &expr[i..i + kw.len()] == *kw {
                if i == 0 || !bytes[i - 1].is_ascii_alphanumeric() {
                    let rest = expr[i + kw.len()..].trim_start();
                    let rest_offset = expr.len() - rest.len();
                    let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(rest.len());
                    if end > 0 {
                        names.insert(rest[..end].to_string());
                    }
                    i = rest_offset + end;
                    break;
                }
            }
        }

        i += 1;
    }
    names
}

fn extract_param_names(params: &str, names: &mut HashSet<String>) {
    for param in params.split(',') {
        let p = param.trim();
        // Handle destructuring roughly — skip `{` and `[` patterns
        if p.starts_with('{') || p.starts_with('[') || p.starts_with("...") {
            continue;
        }
        // Handle `param: Type` and `param = default`
        let name = p.split(|c: char| c == ':' || c == '=').next().unwrap_or("").trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            names.insert(name.to_string());
        }
    }
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
            // Skip member access (identifiers after `.`) but NOT spread `...x`
            if i > 0 && bytes[i - 1] == b'.'
                && !(i >= 3 && bytes[i - 2] == b'.' && bytes[i - 3] == b'.') {
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
                | "Map" | "Set" | "RegExp" | "Symbol" | "BigInt" | "Infinity" | "NaN"
                | "void" | "delete" | "instanceof" | "in" | "of" | "switch" | "case"
                | "break" | "continue" | "throw" | "try" | "catch" | "finally"
                | "for" | "while" | "do" | "async" | "await" | "yield"
                | "satisfies" | "as" | "super" | "with" | "debugger"
                | "default" | "export" | "from") {
                // Skip object literal property names: `{ key: value }`
                // An identifier followed by `:` where the `:` is not part of `::` or `? :`
                let rest_after = &expr[i..];
                let next_non_ws = rest_after.trim_start();
                if next_non_ws.starts_with(':') && !next_non_ws.starts_with("::") {
                    // Check if we're inside an object literal (not a ternary `:`)
                    // Heuristic: if preceded by `{` or `,` at the same brace depth, it's a property name
                    // Simple check: if the identifier is NOT the only thing before `:` in a ternary
                    // context, skip it. This is imperfect but catches most cases.
                    // Only skip if this isn't at top level or after `?`
                    let before = expr[..start].trim_end();
                    if before.ends_with('{') || before.ends_with(',') || before.ends_with('\n') {
                        // Object property name — skip
                    } else {
                        ids.push(id.to_string());
                    }
                } else {
                    ids.push(id.to_string());
                }
            }
            continue;
        }
        i += 1;
    }
    ids
}
