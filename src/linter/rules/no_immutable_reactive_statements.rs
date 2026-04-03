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

        // Also scan module script for imports/consts visible in instance scope
        if let Some(module) = &ctx.ast.module {
            extract_multiline_imports(&module.content, &mut immutable_names);
            for line in module.content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("import ") {
                    for imp in extract_import_names(trimmed) {
                        if let Some(pos) = module.content.find(imp) {
                            immutable_names.insert(&module.content[pos..pos + imp.len()]);
                        }
                    }
                }
            }
        }

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
                } else if trimmed.starts_with("type ") || trimmed.starts_with("export type ")
                    || trimmed.starts_with("interface ") || trimmed.starts_with("export interface ")
                    || trimmed.starts_with("enum ") || trimmed.starts_with("export enum ")
                {
                    // TypeScript type/interface/enum declarations are immutable
                    immutable_names.insert(n);
                } else if trimmed.starts_with("import ") {
                    for imp in extract_import_names(trimmed) {
                        immutable_names.insert(imp);
                    }
                }
            } else {
                // Handle destructuring: `const { a, b } = expr` or `const [a, b] = expr`
                let decl_kw = if trimmed.starts_with("const ") { Some("const ") }
                    else if trimmed.starts_with("let ") { Some("let ") }
                    else if trimmed.starts_with("var ") { Some("var ") }
                    else { None };
                if let Some(kw) = decl_kw {
                    let rest = &trimmed[kw.len()..];
                    if rest.starts_with('{') || rest.starts_with('[') {
                        for dn in extract_destructured_names(rest) {
                            if kw == "const " {
                                const_names.insert(dn);
                            } else {
                                let_names.insert(dn);
                            }
                        }
                    }
                }
                if trimmed.starts_with("import ") {
                    for imp in extract_import_names(trimmed) {
                        immutable_names.insert(imp);
                    }
                }
            }
            // Detect `export { varName as alias }` — makes varName a prop (mutable)
            if (trimmed.starts_with("export {") || trimmed.starts_with("export{"))
                && !trimmed.contains(" from ")
            {
                if let (Some(open), Some(close)) = (trimmed.find('{'), trimmed.find('}')) {
                    for part in trimmed[open+1..close].split(',') {
                        let name = part.trim().split(" as ").next().unwrap_or("").trim();
                        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                            prop_names.insert(name);
                        }
                    }
                }
            }
        }

        // Also extract imports from multi-line import statements
        // (the line-by-line approach above misses these)
        extract_multiline_imports(content, &mut immutable_names);

        // Collect reactive statements with their FULL text (multiline support)
        let reactive_stmts = collect_reactive_stmts(content);

        let is_ts = script.lang.as_deref() == Some("ts")
            || script.lang.as_deref() == Some("typescript");

        // Find mutable let vars (reassigned or bound in template)
        let mut mutable_lets: HashSet<&str> = HashSet::new();
        for &var in &let_names {
            // Check both script content and full source (template event handlers)
            if has_reassignment(content, var) || has_reassignment(ctx.source, var) {
                mutable_lets.insert(var);
            }
        }

        // Classify const variables: immutable unless they have member writes
        // or are bound in template directives
        let mut const_member_written: HashSet<&str> = HashSet::new();

        // Check for member writes in script and template text
        for &var in &const_names {
            if has_member_write(content, var) || has_member_write(ctx.source, var) {
                const_member_written.insert(var);
            }
        }

        // Check bind: directives in template (mutable lets + const member writes)
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                for attr in &el.attributes {
                    if let Attribute::Directive { kind: DirectiveKind::Binding, span, .. } = attr {
                        let region = &ctx.source[span.start as usize..span.end as usize];
                        if let Some(open) = region.find('{') {
                            if let Some(close) = region.find('}') {
                                let expr = region[open+1..close].trim();
                                if let_names.contains(expr) {
                                    mutable_lets.insert(expr);
                                }
                                let base = expr.split('.').next().unwrap_or(expr);
                                if base != expr && let_names.contains(base) {
                                    mutable_lets.insert(base);
                                }
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

        for &var in &const_names {
            if !const_member_written.contains(var) {
                immutable_names.insert(var);
            }
        }

        // Exclude immutable names that are used as {#each} iterables
        // (bind: inside {#each} can mutate the array elements, making the
        // const effectively mutable). But skip names that are shadowed by
        // {@const} declarations in the template — those {#each} blocks
        // reference the local variable, not the script-level one.
        let (each_iterable_names, const_tag_names) = collect_each_and_const_names(&ctx.ast.html);
        immutable_names.retain(|n| {
            // Keep the name unless it's used as an each iterable WITHOUT
            // being shadowed by a {@const} declaration of the same name
            !each_iterable_names.contains(*n) || const_tag_names.contains(*n)
        });

        // Add non-reassigned let vars as immutable (excluding props)
        let all_immutable: HashSet<&str> = immutable_names.iter().copied()
            .chain(let_names.iter()
                .filter(|n| !mutable_lets.contains(*n) && !prop_names.contains(*n))
                .copied())
            .collect();

        // AST-based check: parse script to identify which $: statements
        // truly have all-immutable references (handles TS types properly).
        // Uses the text-based all_immutable set for name-based verification.
        let ast_immutable_stmts = check_immutability_ast(content, is_ts, &all_immutable);

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

            // All referenced declared vars are immutable -> flag
            let text_flag = if has_unknown {
                false // defer to AST
            } else if !referenced.is_empty() {
                referenced.iter().all(|v| all_immutable.contains(v))
            } else if ids.is_empty() && rhs != after {
                // Simple assignment with no reactive identifiers in RHS
                // (e.g., `$: x = false`, `$: x = [...Array(12).keys()]`,
                // `$: x = { key: 'literal' }`). All identifiers were either
                // globals or object property names → not reactive.
                true
            } else {
                false
            };

            // Also check via AST (handles TS types, write-only refs, etc.)
            let ast_flag = if has_unknown || !text_flag {
                let line_num = content[..offset].matches('\n').count();
                ast_immutable_stmts.iter().any(|&s| {
                    let ast_line = content[..s as usize].matches('\n').count();
                    ast_line == line_num
                })
            } else {
                false
            };

            let should_flag = text_flag || ast_flag;
            if should_flag {
                // Match vendor reporting: for simple assignments ($: x = expr),
                // report at the RHS expression; for others, report at the body.
                let after = full_text[2..].trim_start();
                let body_offset_in_stmt = full_text.len() - after.len();
                let (diag_start, diag_end) = if let Some(eq) = after.find('=') {
                    let lhs = after[..eq].trim();
                    let post = &after[eq + 1..];
                    if !post.starts_with('=') && !post.starts_with('>')
                        && lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !lhs.is_empty()
                    {
                        // Simple assignment: report at RHS
                        let rhs_rel = body_offset_in_stmt + eq + 1;
                        let rhs_text = full_text[rhs_rel..].trim_start();
                        let rhs_start = full_text.len() - rhs_text.len();
                        (content_offset + offset + rhs_start, content_offset + offset + full_text.len())
                    } else {
                        // Not simple assignment: report at body
                        (content_offset + offset + body_offset_in_stmt, content_offset + offset + full_text.len())
                    }
                } else {
                    // No assignment: report at body
                    (content_offset + offset + body_offset_in_stmt, content_offset + offset + full_text.len())
                };
                ctx.diagnostic(
                    "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                    oxc::span::Span::new(diag_start as u32, diag_end as u32),
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
            // Skip // line comments (treat as transparent — don't end statement)
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // Skip to end of line AND past the newline
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                if i < bytes.len() { i += 1; } // skip the \n itself
                continue;
            }
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
                // After closing a top-level block, check if it continues
                // with member access (e.g., `{ ... }[key]` or `{ ... }.prop`)
                if depth_brace == 0 && depth_paren == 0 && depth_bracket == 0 {
                    let mut j = i + 1;
                    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') { j += 1; }
                    if j < bytes.len() && (bytes[j] == b'[' || bytes[j] == b'.' || bytes[j] == b'(') {
                        // Continues as member access or call — don't end
                        i += 1;
                        continue;
                    }
                    return j; // end after trailing whitespace
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
                // `=` at end of line (but not `==` or `===`) indicates continuation
                let ends_with_assign = before_nl.ends_with('=')
                    && !before_nl.ends_with("==");
                let continues = before_nl.ends_with('?') || before_nl.ends_with(':')
                    || before_nl.ends_with("||") || before_nl.ends_with("&&")
                    || before_nl.ends_with('+') || before_nl.ends_with('-')
                    || before_nl.ends_with(',') || before_nl.ends_with('\\')
                    || before_nl.ends_with("??")
                    || ends_with_assign
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

/// Extract variable names from destructuring patterns like `{ a, b: c }` or `[a, b]`.
fn extract_destructured_names(pattern: &str) -> Vec<&str> {
    let mut names = Vec::new();
    // Find the closing bracket/brace, then extract identifiers
    let (open, close) = if pattern.starts_with('{') {
        ('{', '}')
    } else {
        ('[', ']')
    };
    if let Some(close_pos) = pattern.find(close) {
        let inner = &pattern[1..close_pos];
        for part in inner.split(',') {
            let part = part.trim();
            if part.starts_with("...") {
                let rest = part[3..].trim();
                let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                    .unwrap_or(rest.len());
                if end > 0 { names.push(&rest[..end]); }
                continue;
            }
            // Handle `key: value` renaming — the value is the local name
            let local = if let Some(colon) = part.find(':') {
                part[colon + 1..].trim()
            } else {
                part
            };
            // Strip type annotations: `name: Type`
            let local = local.split(':').next().unwrap_or(local).trim();
            // Strip default values: `name = default`
            let local = local.split('=').next().unwrap_or(local).trim();
            let end = local.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                .unwrap_or(local.len());
            if end > 0 && !local.is_empty() {
                names.push(&local[..end]);
            }
        }
    }
    names
}

fn extract_decl_name(line: &str) -> Option<&str> {
    let prefixes = ["export const ", "const ", "export let ", "let ", "var ",
                    "export function ", "function ", "export class ", "class ",
                    "export type ", "type ", "export interface ", "interface ",
                    "export enum ", "enum "];
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

/// Extract import names from multi-line import statements in the full content.
fn extract_multiline_imports<'a>(content: &'a str, immutable_names: &mut HashSet<&'a str>) {
    // Find multi-line imports: `import { ... } from '...'` or `import ... from '...'`
    // where the import spans multiple lines (has { on one line, } from on another)
    // Search for "import " at line starts (with possible leading whitespace)
    let mut search = 0;
    while search < content.len() {
        // Find next line containing "import "
        let rest = &content[search..];
        let import_pos = rest.find("import ").or_else(|| rest.find("import\t"));
        let Some(import_pos) = import_pos else { break };
        let abs = search + import_pos;
        // Verify it's at the start of a line (only whitespace before it)
        let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let before_import = content[line_start..abs].trim();
        if !before_import.is_empty() && before_import != "export" {
            search = abs + 7;
            continue;
        }
        // Find the end of this import (the line with ` from ` followed by quote)
        let rest = &content[abs..];
        // Only process if this looks like a multi-line import (has { without } on same line)
        let first_nl = rest.find('\n').unwrap_or(rest.len());
        let first_line = &rest[..first_nl];
        if first_line.contains('{') && !first_line.contains('}') {
            // Multi-line: find closing } from
            // Limit search to 20 lines
            let mut end = abs + first_nl;
            let mut found = false;
            for _ in 0..20 {
                if end >= content.len() { break; }
                let line_end = content[end + 1..].find('\n')
                    .map(|p| end + 1 + p)
                    .unwrap_or(content.len());
                let line = content[end + 1..line_end].trim();
                if line.starts_with("} from ") || line.contains("} from '") || line.contains("} from \"") {
                    end = line_end;
                    found = true;
                    break;
                }
                // If we hit a non-import line, stop
                if !line.starts_with("//") && !line.is_empty() && !line.ends_with(',')
                    && !line.starts_with('}')
                    && line.contains('=') {
                    break;
                }
                end = line_end;
            }
            if found {
                let full_import = &content[abs..end];
                let collapsed: String = full_import.chars()
                    .map(|c| if c == '\n' { ' ' } else { c })
                    .collect();
                for name in extract_import_names(&collapsed) {
                    if let Some(pos) = content[abs..end].find(name) {
                        let actual = &content[abs + pos..abs + pos + name.len()];
                        immutable_names.insert(actual);
                    }
                }
            }
        }
        search = abs + 7; // skip "import "
    }
}

fn extract_import_names(line: &str) -> Vec<&str> {
    let mut names = Vec::new();
    if let Some(from_pos) = line.find(" from ") {
        let mut import_part = line[7..from_pos].trim();
        // Skip `type` keyword in `import type { ... }` (TypeScript type-only import)
        if import_part.starts_with("type ") && import_part[5..].trim_start().starts_with('{') {
            import_part = import_part[5..].trim_start();
        }
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
                    // Strip `type ` prefix from inline type imports (e.g., `type Foo`)
                    let part = part.strip_prefix("type ").unwrap_or(part);
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
                // Skip word boundaries that indicate attribute context (e.g., bind:value=)
                if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b':' { continue; }
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
        if i + 8 < bytes.len() && expr.is_char_boundary(i) && expr.is_char_boundary(i + 8) && &expr[i..i + 8] == "function" {
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
            if i + kw.len() <= bytes.len() && expr.is_char_boundary(i) && expr.is_char_boundary(i + kw.len()) && &expr[i..i + kw.len()] == *kw {
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
        // Skip // line comments
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // Skip to end of line
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        // Skip /* block comments */
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
            if i + 1 < bytes.len() { i += 2; }
            continue;
        }
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

/// AST-based immutability check for reactive statements.
/// Parses script content with oxc and returns the set of `$:` statement byte
/// offsets (within content) where ALL value-level references are immutable.
/// This properly handles TypeScript type annotations (excluded by the AST parser)
/// and function parameters (treated as mutable, matching vendor behavior).
fn check_immutability_ast(content: &str, is_ts: bool, text_immutable: &HashSet<&str>) -> HashSet<u32> {
    use oxc::allocator::Allocator;
    use oxc::ast::AstKind;
    use oxc::parser::Parser;
    use oxc::semantic::SemanticBuilder;
    use oxc::span::{GetSpan, SourceType};

    let mut result = HashSet::new();

    let alloc = Allocator::default();
    let source_type = if is_ts { SourceType::ts() } else { SourceType::mjs() };
    let parse_result = Parser::new(&alloc, content, source_type).parse();
    if parse_result.panicked { return result; }

    let semantic_ret = SemanticBuilder::new().build(&parse_result.program);
    let semantic = semantic_ret.semantic;
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let root_scope = scoping.root_scope_id();

    // Find all LabeledStatement with label "$"
    for node in nodes.iter() {
        let AstKind::LabeledStatement(labeled) = node.kind() else { continue };
        if labeled.label.name.as_str() != "$" { continue; }

        let stmt_start = labeled.span.start;
        let stmt_end = labeled.span.end;

        // Determine if this is a simple assignment: `$: var = expr`
        let is_simple_assign = matches!(
            &labeled.body,
            oxc::ast::ast::Statement::ExpressionStatement(es)
            if matches!(&es.expression, oxc::ast::ast::Expression::AssignmentExpression(_))
        );

        // Collect all value-level identifier references within this statement
        let mut has_any_ref = false;
        let mut all_refs_immutable = true;
        let mut has_store_ref = false;

        for desc in nodes.iter() {
            let AstKind::IdentifierReference(ident) = desc.kind() else { continue };
            let sp = ident.span;
            if sp.start < stmt_start || sp.end > stmt_end { continue; }

            let name = ident.name.as_str();

            // Skip store references ($varName) — they're reactive
            if name.starts_with('$') {
                has_store_ref = true;
                continue;
            }

            // Skip member property access (after `.`)
            let parent_id = nodes.parent_id(desc.id());
            if let AstKind::StaticMemberExpression(member) = nodes.kind(parent_id) {
                if member.property.span == ident.span { continue; }
            }

            // For simple assignments, skip the LHS variable (it's the reactive decl itself)
            if is_simple_assign {
                if let AstKind::AssignmentExpression(assign) = nodes.kind(parent_id) {
                    if assign.left.span().start == ident.span.start { continue; }
                }
            }

            // Skip write-only references (e.g., `writeVar = expr` in a callback)
            if let Some(ref_id) = ident.reference_id.get() {
                let reference = scoping.get_reference(ref_id);
                if reference.is_write() && !reference.is_read() {
                    continue;
                }
            }

            // Resolve the reference
            let symbol = ident.reference_id.get().and_then(|r| scoping.get_reference(r).symbol_id());

            match symbol {
                Some(sym) => {
                    has_any_ref = true;
                    let scope = scoping.symbol_scope_id(sym);
                    if scope != root_scope {
                        // Local variable (function param, arrow param, local const/let)
                        // Skip — these are not reactive references, they're scoped
                        // to the inner function/block and don't affect reactivity
                    } else {
                        // Root-scope variable — check immutability using the
                        // text-based all_immutable set (accounts for member writes,
                        // each-source exclusions, template bindings, etc.)
                        let sym_name = scoping.symbol_name(sym);
                        if !text_immutable.contains(sym_name) {
                            all_refs_immutable = false;
                        }
                    }
                }
                None => {
                    // No symbol binding — could be a JS global (Object, Array, etc.)
                    // or a truly unknown reference. Skip known globals; treat
                    // unknowns as potentially mutable.
                    if !is_known_js_global(name) {
                        all_refs_immutable = false;
                    }
                }
            }
        }

        // If store reference, it's reactive — don't flag
        if has_store_ref { continue; }

        // If all references are immutable root-scope vars, flag it
        if has_any_ref && all_refs_immutable {
            result.insert(stmt_start);
        }
    }

    result
}

/// Collect {#each} iterable names and {@const} tag names in a single template walk.
fn collect_each_and_const_names(fragment: &crate::ast::Fragment) -> (HashSet<String>, HashSet<String>) {
    let mut each_names = HashSet::new();
    let mut const_names = HashSet::new();
    walk_template_nodes(fragment, &mut |node| {
        match node {
            TemplateNode::EachBlock(each) => {
                let expr = each.expression.trim();
                if expr.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !expr.is_empty() {
                    each_names.insert(expr.to_string());
                }
            }
            TemplateNode::ConstTag(ct) => {
                let decl = ct.declaration.trim();
                if let Some(eq) = decl.find('=') {
                    let lhs = decl[..eq].trim();
                    if lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !lhs.is_empty() {
                        const_names.insert(lhs.to_string());
                    }
                }
            }
            _ => {}
        }
    });
    (each_names, const_names)
}

fn is_known_js_global(name: &str) -> bool {
    matches!(name,
        "Object" | "Array" | "String" | "Number" | "Boolean" | "Date" | "Error"
        | "Promise" | "Map" | "Set" | "WeakMap" | "WeakSet" | "RegExp" | "Symbol"
        | "BigInt" | "Math" | "JSON" | "Infinity" | "NaN" | "undefined"
        | "parseInt" | "parseFloat" | "isNaN" | "isFinite"
        | "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent"
        | "console" | "globalThis"
        | "Proxy" | "Reflect" | "WeakRef"
        | "ArrayBuffer" | "SharedArrayBuffer" | "DataView"
        | "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array"
        | "Uint32Array" | "Int32Array" | "Float32Array" | "Float64Array"
        | "Intl" | "Atomics"
    )
}
