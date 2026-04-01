//! `svelte/infinite-reactive-loop` — detect reactive statements that may cause infinite loops.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct InfiniteReactiveLoop;

impl Rule for InfiniteReactiveLoop {
    fn name(&self) -> &'static str {
        "svelte/infinite-reactive-loop"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let script = match &ctx.ast.instance {
            Some(s) => s,
            None => return,
        };
        let content = &script.content;
        let base = script.span.start as usize;
        let source = ctx.source;
        let tag_text = &source[base..script.span.end as usize];
        let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

        let mut top_vars = collect_top_level_vars(content);
        // Also collect vars from module script (context="module")
        if let Some(module) = &ctx.ast.module {
            let mut module_vars = collect_top_level_vars(&module.content);
            top_vars.append(&mut module_vars);
        }
        collect_store_refs(content, &mut top_vars);
        let aliases = collect_aliases(content);
        let func_info = collect_func_info(content, &top_vars);

        let mut search_pos = 0;
        while let Some((bs, be)) = find_reactive_block(content, search_pos) {
            let block = &content[bs..be];
            analyze_block(ctx, block, bs, content_offset, &top_vars, &aliases, &func_info);
            search_pos = be;
        }
    }
}

fn collect_top_level_vars(content: &str) -> Vec<String> {
    let mut vars = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        // Handle `export let` and `export var` in addition to `let` and `var`
        let t = t.strip_prefix("export ").unwrap_or(t);
        for kw in &["let ", "var ", "const "] {
            if let Some(rest) = t.strip_prefix(kw) {
                extract_declared_names(rest, &mut vars);
            }
        }
        // Also collect variables declared via $: reactive assignment
        // (e.g. `$: icon = expr` where there's no prior `let icon`)
        if t.starts_with("$:") {
            let after = t[2..].trim_start();
            if let Some(eq_pos) = after.find('=') {
                let name = after[..eq_pos].trim();
                // Skip if it starts with `{` or `[` (destructuring), or contains special chars
                if !name.is_empty()
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                    && !name.starts_with('{') && !name.starts_with('[')
                {
                    // Skip if `=` is followed by `=` (comparison) or preceded by `!` or `>` or `<`
                    let post_eq = &after[eq_pos + 1..];
                    if !post_eq.starts_with('=') && !post_eq.starts_with('>') {
                        if !vars.contains(&name.to_string()) {
                            vars.push(name.to_string());
                        }
                    }
                }
            }
        }
    }
    vars
}

/// Extract variable names from a declaration's right-hand side.
/// Handles: simple (`a = 0`), multi (`a = 0, b = 1`), destructured (`{ a, b } = ...`, `[a, b] = ...`).
fn extract_declared_names(decl: &str, vars: &mut Vec<String>) {
    let trimmed = decl.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Destructured: extract identifiers between braces/brackets up to `}`/`]`
        let close = if trimmed.starts_with('{') { '}' } else { ']' };
        if let Some(end) = trimmed.find(close) {
            let inner = &trimmed[1..end];
            for part in inner.split(',') {
                let p = part.trim();
                // Handle renaming: `orig: alias` → take alias
                let name_part = if let Some(colon) = p.find(':') {
                    p[colon + 1..].trim()
                } else {
                    p
                };
                // Handle defaults: `name = default` → take name
                let name_part = if let Some(eq) = name_part.find('=') {
                    name_part[..eq].trim()
                } else {
                    name_part
                };
                // Skip rest elements (`...rest` → `rest`)
                let name_part = name_part.strip_prefix("...").unwrap_or(name_part);
                let name: String = name_part.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() { vars.push(name); }
            }
        }
    } else {
        // Simple or multi-variable: `a = 0, b = 1` or just `a = 0`
        // Split by commas that are at depth 0 (not inside parens/brackets/braces)
        let mut depth = 0i32;
        let mut seg_start = 0;
        let bytes = trimmed.as_bytes();
        for i in 0..=bytes.len() {
            let at_end = i == bytes.len();
            let is_comma = !at_end && bytes[i] == b',' && depth == 0;
            if at_end || is_comma {
                let seg = trimmed[seg_start..i].trim();
                let name: String = seg.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() { vars.push(name); }
                seg_start = i + 1;
            } else {
                match bytes[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    _ => {}
                }
            }
        }
    }
}

fn collect_store_refs(content: &str, vars: &mut Vec<String>) {
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("import ") {
            if let (Some(bs), Some(be)) = (t.find('{'), t.find('}')) {
                for imp in t[bs+1..be].split(',') {
                    let imp = imp.trim();
                    let name = if let Some(ap) = imp.find(" as ") {
                        imp[ap+4..].trim()
                    } else { imp };
                    if !name.is_empty() {
                        let sr = format!("${}", name);
                        if content.contains(&sr) && !vars.contains(&sr) {
                            vars.push(sr);
                        }
                    }
                }
            }
        }
    }
}

fn collect_aliases(content: &str) -> Vec<(String, String)> {
    let fns = ["setTimeout", "setInterval", "queueMicrotask", "tick"];
    let mut aliases = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("const ") {
            if let Some(eq) = rest.find(" = ") {
                let alias = &rest[..eq];
                let val = rest[eq+3..].trim().trim_end_matches(';');
                for &f in &fns {
                    if val == f { aliases.push((f.to_string(), alias.to_string())); }
                }
            }
        }
        if t.starts_with("import ") {
            if let (Some(bs), Some(be)) = (t.find('{'), t.find('}')) {
                for imp in t[bs+1..be].split(',') {
                    let imp = imp.trim();
                    if let Some(ap) = imp.find(" as ") {
                        let orig = imp[..ap].trim();
                        let alias = imp[ap+4..].trim();
                        for &f in &fns {
                            if orig == f { aliases.push((f.to_string(), alias.to_string())); }
                        }
                    }
                }
            }
        }
    }
    aliases
}

struct FuncInfo {
    name: String,
    assigns: Vec<String>,
    has_await: bool,
    assigns_after_await: Vec<String>,
    /// Byte positions of assignment lines after await (relative to content start)
    assign_positions_after_await: Vec<(String, usize)>,
    /// Byte positions of ALL assignment lines (relative to content start)
    all_assign_positions: Vec<(String, usize)>,
    /// Function names called after await (for transitive propagation)
    calls_after_await: Vec<String>,
    /// Function names called anywhere in the body
    all_calls: Vec<String>,
    /// Byte range of the function body within the script content (start, end)
    body_range: (usize, usize),
}

fn collect_func_info(content: &str, top_vars: &[String]) -> Vec<FuncInfo> {
    let mut results = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Pre-compute line byte offsets and brace depths
    let mut line_offsets = Vec::with_capacity(lines.len());
    let mut line_depths = Vec::with_capacity(lines.len());
    let mut depth = 0i32;
    let mut offset = 0usize;
    for &line in &lines {
        line_offsets.push(offset);
        line_depths.push(depth);
        for ch in line.bytes() {
            match ch { b'{' => depth += 1, b'}' => depth -= 1, _ => {} }
        }
        offset += line.len() + 1;
    }

    // Pre-collect all function names for call tracking
    let func_names_seen: Vec<String> = lines.iter().enumerate()
        .filter(|(idx, _)| line_depths[*idx] == 0)
        .filter_map(|(_, line)| extract_func_name(line.trim()))
        .collect();

    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        if line_depths[i] > 0 { i += 1; continue; }
        if let Some(name) = extract_func_name(t) {
            // Find function body range (line indices)
            let mut depth = 0i32;
            let mut started = false;
            let mut end_line = i;
            let mut has_await = false;
            for j in i..lines.len() {
                let line = lines[j];
                if line.contains("await ") { has_await = true; }
                for ch in line.bytes() {
                    if ch == b'{' { depth += 1; started = true; }
                    if ch == b'}' { depth -= 1; }
                }
                end_line = j;
                if started && depth <= 0 { break; }
            }
            let func_start_offset = line_offsets[i];

            // Single pass: collect assigns, positions, and function calls
            let mut assigns = Vec::new();
            let mut all_assign_positions = Vec::new();
            let mut assigns_after_await = Vec::new();
            let mut assign_positions_after_await = Vec::new();
            let mut calls_after_await = Vec::new();
            let mut all_calls = Vec::new();
            let mut seen_await = false;

            for j in i..=end_line {
                let line = lines[j];
                let t = line.trim();
                let indent = line.len() - t.len();
                let line_pos = line_offsets[j];

                // Skip the declaration line itself — `const name = () => {` is not
                // an assignment to `name` inside the function body.
                if j == i { continue; }

                if t.contains("await ") {
                    // Check for `var = await expr` on this line
                    if seen_await || j > i {
                        for var in top_vars {
                            if var == &name { continue; }
                            let pat = format!("{} = await ", var);
                            if t.contains(&pat) {
                                if !assigns_after_await.contains(var) { assigns_after_await.push(var.clone()); }
                                assign_positions_after_await.push((var.clone(), line_pos + indent));
                                if !assigns.contains(var) { assigns.push(var.clone()); }
                                all_assign_positions.push((var.clone(), line_pos + indent));
                            }
                        }
                    }
                    seen_await = true;
                }

                // Check assignments on this line (skip local declarations)
                for var in top_vars {
                    if var == &name { continue; }
                    if !t.contains(var.as_str()) { continue; }
                    // Skip if the variable appears in a local declaration
                    if is_local_declaration(t, var) { continue; }
                    if has_assign(t, var) {
                        if !assigns.contains(var) { assigns.push(var.clone()); }
                        all_assign_positions.push((var.clone(), line_pos + indent));
                        if seen_await {
                            if !assigns_after_await.contains(var) { assigns_after_await.push(var.clone()); }
                            assign_positions_after_await.push((var.clone(), line_pos + indent));
                        }
                    }
                }

                // Track function calls on this line (for transitive propagation)
                // Look for `funcName(` patterns that aren't the function itself
                for other in &func_names_seen {
                    if other == &name { continue; }
                    let call_pat = format!("{}(", other);
                    if t.contains(&call_pat) {
                        if !all_calls.contains(other) { all_calls.push(other.clone()); }
                        if seen_await && !calls_after_await.contains(other) {
                            calls_after_await.push(other.clone());
                        }
                    }
                }
            }

            let body_start = func_start_offset;
            let body_end = if end_line < line_offsets.len() { line_offsets[end_line] + lines[end_line].len() } else { content.len() };
            results.push(FuncInfo { name, assigns, has_await, assigns_after_await, assign_positions_after_await, all_assign_positions, calls_after_await, all_calls, body_range: (body_start, body_end) });
        }
        i += 1;
    }

    // Propagation pass: if func A calls func B (after await or always), inherit B's assigns.
    // This handles chains like: backgroundResendVerification → resetTurnstile → turnstileReady = false
    // Run multiple rounds to handle deeper chains.
    for _ in 0..4 {
        let snapshot: Vec<(String, Vec<String>, Vec<(String, usize)>, Vec<String>, Vec<(String, usize)>, bool)> = results.iter()
            .map(|fi| (fi.name.clone(), fi.assigns.clone(), fi.all_assign_positions.clone(), fi.assigns_after_await.clone(), fi.assign_positions_after_await.clone(), fi.has_await))
            .collect();

        for fi in results.iter_mut() {
            // For calls after await: inherit callee's assigns based on
            // whether the callee is async:
            // - If callee has_await: only inherit assigns_after_await
            //   (callee's pre-await code runs synchronously in same microtask)
            // - If callee !has_await: inherit ALL assigns
            //   (entire callee runs in a different microtask)
            for callee_name in fi.calls_after_await.clone() {
                if let Some((_, callee_assigns, callee_positions, callee_after_await, callee_after_await_positions, callee_has_await)) = snapshot.iter().find(|(n, _, _, _, _, _)| n == &callee_name) {
                    let (vars_to_inherit, positions_to_inherit): (&Vec<String>, &Vec<(String, usize)>) = if *callee_has_await {
                        (callee_after_await, callee_after_await_positions)
                    } else {
                        (callee_assigns, callee_positions)
                    };
                    for var in vars_to_inherit {
                        if !fi.assigns_after_await.contains(var) {
                            fi.assigns_after_await.push(var.clone());
                        }
                    }
                    for var in callee_assigns {
                        if !fi.assigns.contains(var) {
                            fi.assigns.push(var.clone());
                        }
                    }
                    for (var, pos) in positions_to_inherit {
                        if !fi.assign_positions_after_await.iter().any(|(v, p)| v == var && p == pos) {
                            fi.assign_positions_after_await.push((var.clone(), *pos));
                        }
                    }
                    for (var, pos) in callee_positions {
                        if !fi.all_assign_positions.iter().any(|(v, p)| v == var && p == pos) {
                            fi.all_assign_positions.push((var.clone(), *pos));
                        }
                    }
                }
            }
            // For all calls: inherit callee's assigns into our assigns list
            for callee_name in fi.all_calls.clone() {
                if let Some((_, callee_assigns, callee_positions, _, _, _)) = snapshot.iter().find(|(n, _, _, _, _, _)| n == &callee_name) {
                    for var in callee_assigns {
                        if !fi.assigns.contains(var) {
                            fi.assigns.push(var.clone());
                        }
                    }
                    for (var, pos) in callee_positions {
                        if !fi.all_assign_positions.iter().any(|(v, p)| v == var && p == pos) {
                            fi.all_assign_positions.push((var.clone(), *pos));
                        }
                    }
                }
            }
        }
    }

    results
}

/// Collect variables assigned after an `await` in a function body.
/// Also returns (var_name, content_offset) pairs for each assignment.
fn extract_func_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("const ") {
        if let Some(eq) = rest.find(" = ") {
            let name = &rest[..eq];
            let after = rest[eq+3..].trim();
            if is_direct_function_expr(after) {
                return Some(name.to_string());
            }
        }
    }
    if let Some(rest) = line.strip_prefix("function ") {
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if !name.is_empty() { return Some(name); }
    }
    if let Some(rest) = line.strip_prefix("async function ") {
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if !name.is_empty() { return Some(name); }
    }
    // Also handle `let name = async ...` and `let name = (...) => { ... }`
    if let Some(rest) = line.strip_prefix("let ") {
        if let Some(eq) = rest.find(" = ") {
            let name = &rest[..eq];
            let after = rest[eq+3..].trim();
            if is_direct_function_expr(after) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Check if `after` (the text after `= `) is a direct function/arrow expression,
/// NOT a call expression that happens to contain an arrow (e.g. `debounce(() => ...)`).
fn is_direct_function_expr(after: &str) -> bool {
    if after.starts_with("function") || after.starts_with("async") {
        return true;
    }
    if after.starts_with('(') {
        // Scan for the matching `)` at depth 0, then check if `=>` follows
        let mut depth = 0i32;
        for (i, ch) in after.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let rest = after[i + 1..].trim_start();
                        return rest.starts_with("=>");
                    }
                }
                _ => {}
            }
        }
        return false;
    }
    // Single-param arrow without parens: `x => ...`
    let ident_end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
    if ident_end > 0 {
        let rest = after[ident_end..].trim_start();
        if rest.starts_with("=>") {
            return true;
        }
    }
    false
}

/// Check if a line is a local variable declaration (const/let/var) for the given variable.
fn is_local_declaration(line: &str, var: &str) -> bool {
    for kw in &["const ", "let ", "var "] {
        if let Some(kw_pos) = line.find(kw) {
            let after_kw = &line[kw_pos + kw.len()..];
            // Check destructuring: const { x, y } = or const [x, y] =
            if after_kw.starts_with('{') || after_kw.starts_with('[') {
                if let Some(close) = after_kw.find(|c: char| c == '}' || c == ']') {
                    let inside = &after_kw[1..close];
                    if inside.split(',').any(|part| part.trim() == var) {
                        return true;
                    }
                }
            } else {
                // Simple declaration: const var = ... or const var: Type = ...
                let name_end = after_kw.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                    .unwrap_or(after_kw.len());
                if &after_kw[..name_end] == var {
                    return true;
                }
            }
        }
    }
    false
}

fn has_assign(line: &str, var: &str) -> bool {
    // Fast path: if the variable name doesn't appear at all, skip
    if !line.contains(var) { return false; }
    has_assign_with_patterns_inline(line, var)
}

fn has_assign_with_patterns_inline(line: &str, var: &str) -> bool {
    let ops = [" = ", " += ", " -= ", " *= ", " /= "];
    for op in &ops {
        // Inline pattern check without format! allocation
        if let Some(pos) = find_var_op(line, var, op) {
            if *op == " = " && pos + var.len() + op.len() < line.len()
                && line.as_bytes()[pos + var.len() + op.len()] == b'=' { continue; }
            return true;
        }
    }
    // Also check for assignment at end of line: `var =` (newline after =)
    // This handles multi-line assignments where the value is on the next line
    if let Some(pos) = find_var_op(line, var, " =") {
        let after = pos + var.len() + 2;
        // Make sure it's not == or =>
        if after >= line.len() || {
            let b = line.as_bytes()[after];
            b != b'=' && b != b'>'
        } {
            return true;
        }
    }
    // Check property/index access
    has_member_assign(line, var, &ops)
}

/// Fast check for `var` + `op` at a word boundary, without allocation.
fn find_var_op(line: &str, var: &str, op: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = line[start..].find(var) {
        let abs = start + pos;
        if is_word_start(line, abs) {
            let after = abs + var.len();
            if line[after..].starts_with(op) {
                return Some(abs);
            }
        }
        start = abs + 1;
    }
    None
}

fn has_member_assign(line: &str, var: &str, ops: &[&str]) -> bool {
    for &sep in &[".", "["] {
        let mut start = 0;
        let prefix_len = var.len() + sep.len();
        while start + prefix_len <= line.len() {
            let pos = match line[start..].find(var) {
                Some(p) => start + p,
                None => break,
            };
            if !is_word_start(line, pos) { start = pos + 1; continue; }
            let after_var = pos + var.len();
            if !line[after_var..].starts_with(sep) { start = pos + 1; continue; }

            let rest = &line[after_var + sep.len()..];
            let mut r = if sep == "." {
                let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
                &rest[end..]
            } else {
                rest.find(']').map(|p| &rest[p+1..]).unwrap_or("")
            };
            loop {
                if r.starts_with('[') {
                    if let Some(close) = r.find(']') { r = &r[close+1..]; } else { break; }
                } else if r.starts_with('.') {
                    let end = r[1..].find(|c: char| !c.is_alphanumeric() && c != '_').map(|e| e+1).unwrap_or(r.len());
                    r = &r[end..];
                } else { break; }
            }
            let r = r.trim_start();
            for op in ops {
                if r.starts_with(op.trim()) {
                    if *op == " = " && r.len() > 1 && r.as_bytes()[1] == b'=' { continue; }
                    return true;
                }
            }
            start = pos + 1;
        }
    }
    false
}

fn is_word_start(text: &str, pos: usize) -> bool {
    if pos == 0 { return true; }
    let b = text.as_bytes()[pos - 1];
    !(b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

/// Check if an assignment at `assign_pos` is after an await at `await_pos` on the same line,
/// and the assignment actually executes after the await completes.
fn is_after_await_same_line(line: &str, await_pos: usize, assign_pos: usize) -> bool {
    if assign_pos <= await_pos { return false; }

    // Check if there's a comma separator between await and assignment
    // Pattern: `(await expr, (assignment))` or `(await expr, assignment)`
    let between = &line[await_pos..assign_pos];
    if between.contains(',') {
        return true;
    }

    false
}

/// Find the position of an assignment to `var` in the line.
fn find_assign_pos(line: &str, var: &str) -> Option<usize> {
    let ops = [" = ", " += ", " -= "];
    for op in &ops {
        let pat = format!("{}{}", var, op);
        if let Some(pos) = line.find(&pat) {
            if is_word_start(line, pos) {
                if *op == " = " && pos + pat.len() < line.len() && line.as_bytes()[pos + pat.len()] == b'=' {
                    continue;
                }
                return Some(pos);
            }
        }
        let prop_pat = format!("{}.", var);
        for (pos, _) in line.match_indices(&prop_pat) {
            if !is_word_start(line, pos) { continue; }
            let rest = &line[pos + prop_pat.len()..];
            if rest.contains(op) { return Some(pos); }
        }
    }
    None
}

fn find_reactive_block(content: &str, from: usize) -> Option<(usize, usize)> {
    let remaining = &content[from..];
    let mut ls = 0usize;
    for line in remaining.lines() {
        let t = line.trim();
        if t.starts_with("$:") {
            let abs = from + ls + (line.len() - t.len());
            let end = find_block_end(content, abs);
            return Some((abs, end));
        }
        ls += line.len() + 1;
    }
    None
}

fn find_block_end(content: &str, dollar: usize) -> usize {
    let after = &content[dollar + 2..];
    let trimmed = after.trim_start();
    if trimmed.starts_with('{') {
        find_matching_brace(content, dollar + 2 + (after.len() - trimmed.len()))
    } else {
        find_stmt_end(content, dollar)
    }
}

fn find_matching_brace(content: &str, start: usize) -> usize {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut sch = '"';
    for (i, ch) in content[start..].char_indices() {
        if in_str {
            if ch == sch && content.as_bytes().get(start + i - 1) != Some(&b'\\') { in_str = false; }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => { in_str = true; sch = ch; }
            '{' => depth += 1,
            '}' => { depth -= 1; if depth == 0 { return start + i + 1; } }
            _ => {}
        }
    }
    content.len()
}

fn find_stmt_end(content: &str, dollar: usize) -> usize {
    let mut pdepth = 0i32;
    let mut bdepth = 0i32;
    let mut in_str = false;
    let mut sch = '"';
    let mut has_c = false;
    for (i, ch) in content[dollar + 2..].char_indices() {
        let abs = dollar + 2 + i;
        if in_str {
            if ch == sch && content.as_bytes().get(abs - 1) != Some(&b'\\') { in_str = false; }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => { in_str = true; sch = ch; has_c = true; }
            '{' => { bdepth += 1; has_c = true; }
            '}' => {
                bdepth -= 1;
                // If we close the outermost brace AND we're not inside parens,
                // check if followed by `else` (if-else blocks continue past the first })
                if bdepth <= 0 && pdepth <= 0 && has_c {
                    let rest = content[abs + 1..].trim_start();
                    if rest.starts_with("else") {
                        // Continue past the else block
                    } else {
                        return abs + 1;
                    }
                }
            }
            '(' => { pdepth += 1; has_c = true; }
            ')' => {
                pdepth -= 1;
                if pdepth <= 0 && bdepth <= 0 && has_c {
                    let rest = &content[abs + 1..];
                    let nt = rest.trim_start_matches(|c: char| c == ' ' || c == '\t');
                    if nt.starts_with(';') {
                        return abs + 1 + (rest.len() - nt.len()) + 1;
                    }
                    if nt.starts_with('\n') || nt.is_empty() {
                        // Check if the next non-whitespace token is a continuation
                        // (method chain, ternary, logical operator, etc.)
                        let after_ws = nt.trim_start();
                        let is_continuation = after_ws.starts_with('.')
                            || after_ws.starts_with('?')
                            || after_ws.starts_with(':')
                            || after_ws.starts_with('+')
                            || after_ws.starts_with('-')
                            || after_ws.starts_with('*')
                            || after_ws.starts_with('/')
                            || after_ws.starts_with('%')
                            || after_ws.starts_with('&')
                            || after_ws.starts_with('|')
                            || after_ws.starts_with('^')
                            || after_ws.starts_with('<')
                            || after_ws.starts_with('>')
                            || after_ws.starts_with('=')
                            || after_ws.starts_with('!')
                            || after_ws.starts_with(',')
                            || after_ws.starts_with('(');
                        if !is_continuation {
                            return abs + 1 + (rest.len() - nt.len());
                        }
                    }
                }
            }
            ';' if pdepth <= 0 && bdepth <= 0 && has_c => return abs + 1,
            '\n' if pdepth <= 0 && bdepth <= 0 && has_c => {
                // ASI-like check: if the next non-blank line starts with a
                // statement keyword or declaration, end the statement here.
                let rest = &content[abs + 1..];
                let next_line = rest.trim_start();
                if next_line.is_empty() {
                    return abs + 1;
                }
                // Blank line followed by content → end statement
                if rest.starts_with('\n') || (rest.starts_with("\r\n") && !next_line.is_empty()) {
                    return abs + 1;
                }
                // Statement-starting keywords
                let stmt_starts = ["async ", "function ", "const ", "let ", "var ",
                    "if ", "if(", "for ", "for(", "while ", "while(", "return ",
                    "return;", "throw ", "class ", "import ", "export ",
                    "try ", "try{", "switch ", "switch(", "$: "];
                for kw in &stmt_starts {
                    if next_line.starts_with(kw) {
                        return abs + 1;
                    }
                }
            }
            _ if !ch.is_whitespace() => has_c = true,
            _ => {}
        }
    }
    content.len()
}

/// Check if a variable name appears anywhere in the block (read or write) with word boundaries.
fn block_has_var_ref(block: &str, var: &str) -> bool {
    for (pos, _) in block.match_indices(var) {
        if is_word_start(block, pos) {
            let after = pos + var.len();
            if after >= block.len() || {
                let b = block.as_bytes()[after];
                !b.is_ascii_alphanumeric() && b != b'_' && b != b'$'
            } {
                return true;
            }
        }
    }
    false
}

/// Check if a block reads a variable (not just assigns to it).
/// Compound assignments (+=, -=) count as reads. Only simple `var = expr` is write-only.
fn block_reads_var(block: &str, var: &str) -> bool {
    for line in block.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("$:") { continue; }
        if !t.contains(var) { continue; }

        let count = t.match_indices(var)
            .filter(|(pos, _)| is_word_start(t, *pos))
            .count();
        if count == 0 { continue; }

        // Multiple occurrences means at least one is a read
        if count > 1 { return true; }

        // Single occurrence: check if it's ONLY a simple assignment (write-only)
        // Compound assignments (+=, -=, etc.) are read-modify-write → count as reads
        let simple_assign = {
            let pat = format!("{} = ", var);
            if let Some(pos) = t.find(&pat) {
                is_word_start(t, pos) && {
                    let after = &t[pos + pat.len()..];
                    !after.starts_with('=') // not ==
                }
            } else {
                false
            }
        };
        // Also check var.prop = and var[idx] = as simple writes
        let member_assign = {
            let dot_pat = format!("{}.", var);
            let idx_pat = format!("{}[", var);
            (t.contains(&dot_pat) || t.contains(&idx_pat)) && has_assign(t, var)
        };
        if simple_assign || member_assign {
            continue; // write-only
        }

        // If has_assign but not simple assign, it's a compound assignment (read)
        // Or it's a non-assignment reference (read)
        return true;
    }
    false
}

/// Position-based analysis: check if a byte position in the block is inside a
/// callback passed to a .then(), .catch(), setTimeout, etc. or after an await.
/// Uses backward scanning through the raw text.
fn is_in_async_callback(block: &str, pos: usize, all_triggers: &[String]) -> bool {
    let before = &block[..pos.min(block.len())];
    // Scan backwards tracking paren depth to find the enclosing function call
    let mut paren_depth = 0i32;
    let bytes = before.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => paren_depth += 1,
            b'(' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                } else {
                    // Unmatched ( - check what precedes it
                    let before_paren = before[..i].trim_end();
                    // .then( or .catch(
                    if before_paren.ends_with(".then") || before_paren.ends_with(".catch") {
                        return true;
                    }
                    // trigger functions
                    for trigger in all_triggers {
                        if before_paren.ends_with(trigger.as_str()) {
                            let tlen = trigger.len();
                            let start = before_paren.len() - tlen;
                            if start == 0 || {
                                let b = before_paren.as_bytes()[start - 1];
                                !b.is_ascii_alphanumeric() && b != b'_' && b != b'$'
                            } {
                                return true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Check if there's an `await` before `pos` in the block at the same scope depth,
/// on a PRECEDING line. Uses brace-depth tracking to ignore awaits in nested functions.
fn has_await_on_prev_line(block: &str, line_start_pos: usize) -> bool {
    // We need to find an `await` keyword that:
    // 1. Is on a previous line (before line_start_pos)
    // 2. Is at the same brace depth as our position
    // 3. Is inside an async function context

    let before = &block[..line_start_pos.min(block.len())];

    // First check: is there any async context?
    if !before.contains("async ") && !before.contains("async(") {
        return false;
    }

    // Track brace depth from the start. Record the depth at our position.
    let mut depth = 0i32;
    let mut in_str = false;
    let mut sch = '"';
    let bytes = block.as_bytes();

    // First pass: find brace depth at line_start_pos
    let mut target_depth = 0i32;
    for i in 0..line_start_pos.min(bytes.len()) {
        let ch = bytes[i] as char;
        if in_str {
            if ch == sch && (i == 0 || bytes[i-1] != b'\\') { in_str = false; }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => { in_str = true; sch = ch; }
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    target_depth = depth;

    // Find the depth of the innermost enclosing async function body.
    // This is the depth right after `async ... {` — await at this depth or deeper
    // within the same function is relevant.
    let async_body_depth = {
        let mut d = 0i32;
        let mut is = false;
        let mut sc = '"';
        let mut best = 0i32;
        let b = &block[..line_start_pos.min(block.len())];
        let mut j = 0;
        while j < b.len() {
            let c = b.as_bytes()[j] as char;
            if is { if c == sc && (j == 0 || b.as_bytes()[j-1] != b'\\') { is = false; } j += 1; continue; }
            match c {
                '\'' | '"' | '`' => { is = true; sc = c; }
                '{' => d += 1,
                '}' => d -= 1,
                _ => {}
            }
            // Check for `async ` followed eventually by `{`
            if j + 6 <= b.len() && &b[j..j+6] == "async " {
                // This async's body depth = d + 1 (the { that follows)
                best = d + 1;
            }
            j += 1;
        }
        best
    };

    // Second pass: find `await` keywords on preceding lines
    depth = 0;
    in_str = false;
    sch = '"';
    let mut last_newline = 0usize;
    let mut found_await_at_depth = false;

    for i in 0..line_start_pos.min(bytes.len()) {
        let ch = bytes[i] as char;
        if in_str {
            if ch == sch && (i == 0 || bytes[i-1] != b'\\') { in_str = false; }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => { in_str = true; sch = ch; }
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if ch == '\n' {
            last_newline = i + 1;
        }

        // Check for `await ` at or above the target depth (within the same async function).
        // Code in if/else branches after await in the same async function is still
        // in a different microtask.
        if depth >= async_body_depth && depth <= target_depth
            && i + 6 <= before.len() && &before[i..i+6] == "await " {
            if i == 0 || !bytes[i-1].is_ascii_alphanumeric() {
                if i < line_start_pos {
                    found_await_at_depth = true;
                }
            }
        }
    }

    found_await_at_depth
}

/// Find byte ranges of .then() and .catch() callback bodies in the block.
/// Returns Vec<(body_start, body_end)> where body is the content of the callback.
/// Handles both block bodies `(x) => { ... }` and expression bodies `(x) => expr`.
fn find_then_catch_regions(block: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let bytes = block.as_bytes();

    // Find all `.then(` and `.catch(` positions
    for pattern in &[".then(", ".catch("] {
        let mut search = 0;
        while let Some(pos) = block[search..].find(pattern) {
            let abs = search + pos;
            let after = abs + pattern.len();

            let mut i = after;
            let mut paren_depth = 1i32;
            let mut body_start = None;
            let mut body_end = None;
            let mut found_arrow = false;

            while i < bytes.len() && paren_depth > 0 {
                match bytes[i] {
                    b'\'' | b'"' | b'`' => {
                        let q = bytes[i];
                        i += 1;
                        while i < bytes.len() {
                            if bytes[i] == b'\\' { i += 1; }
                            else if bytes[i] == q { break; }
                            i += 1;
                        }
                    }
                    b'(' => paren_depth += 1,
                    b')' => {
                        paren_depth -= 1;
                        if paren_depth == 0 && body_start.is_some() && body_end.is_none() {
                            body_end = Some(i);
                        }
                    }
                    b'=' if !found_arrow && i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                        found_arrow = true;
                        i += 2;
                        // Skip whitespace after =>
                        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n' || bytes[i] == b'\r') {
                            i += 1;
                        }
                        if i < bytes.len() && bytes[i] != b'{' {
                            // Expression body (no block): region from here to end of .then()
                            body_start = Some(i);
                        }
                        // For block body, let the `{` handler below set body_start
                        continue;
                    }
                    b'{' if body_start.is_none() && paren_depth >= 1 => {
                        body_start = Some(i);
                    }
                    _ => {}
                }
                i += 1;
            }

            if let (Some(bs), Some(be)) = (body_start, body_end) {
                regions.push((bs, be));
            }

            search = abs + pattern.len();
        }
    }

    regions
}

fn analyze_block(
    ctx: &mut LintContext,
    block: &str,
    block_start: usize,
    base: usize,
    top_vars: &[String],
    aliases: &[(String, String)],
    func_info: &[FuncInfo],
) {
    let trigger_fns = ["setTimeout", "setInterval", "queueMicrotask", "tick"];
    let mut all_triggers: Vec<String> = trigger_fns.iter().map(|s| s.to_string()).collect();
    for (orig, alias) in aliases {
        if trigger_fns.contains(&orig.as_str()) {
            all_triggers.push(alias.clone());
        }
    }

    // Check if tick is locally redefined in the script (not just the block)
    // For tick02 test: `function tick(fn) { fn(); }` means tick is not an async trigger

    // Collect locally declared names in this block (for variable shadowing)
    let local_names: Vec<String> = block.lines()
        .filter_map(|line| {
            let t = line.trim();
            for kw in &["let ", "const ", "var "] {
                if let Some(rest) = t.strip_prefix(kw) {
                    let name: String = rest.chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                        .collect();
                    if !name.is_empty() { return Some(name); }
                }
            }
            if let Some(rest) = t.strip_prefix("function ") {
                let name: String = rest.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() { return Some(name); }
            }
            None
        })
        .collect();

    // Pre-compute .then()/.catch() callback regions in the block
    let async_callback_regions = find_then_catch_regions(block);

    // Compute tracked (dependency) variables for this reactive block.
    // Only variables that are READ inside the block can re-trigger it.
    // This matches the vendor's getTrackedVariableNodes approach.
    let tracked_vars: std::collections::HashSet<String> = {
        let mut tracked = std::collections::HashSet::new();
        let bytes = block.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            // Skip strings
            if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
                let q = bytes[i]; i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' { i += 1; }
                    i += 1;
                }
                if i < bytes.len() { i += 1; }
                continue;
            }
            // Skip comments
            if bytes[i] == b'/' && i + 1 < bytes.len() {
                if bytes[i+1] == b'/' { while i < bytes.len() && bytes[i] != b'\n' { i += 1; } continue; }
                if bytes[i+1] == b'*' { i += 2; while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i+1] == b'/') { i += 1; } i += 2; continue; }
            }
            if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$' {
                // Skip member access (after `.`)
                if i > 0 && bytes[i-1] == b'.'
                    && !(i >= 3 && bytes[i-2] == b'.' && bytes[i-3] == b'.') {
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                    continue;
                }
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                let ident = &block[start..i];
                // Only track top-level variables (not JS keywords, locals, etc.)
                if top_vars.iter().any(|v| v == ident) && !local_names.contains(&ident.to_string()) {
                    tracked.insert(ident.to_string());
                }
            } else {
                i += 1;
            }
        }
        tracked
    };

    let lines: Vec<&str> = block.lines().collect();
    let mut line_offsets = Vec::new();
    let mut off = 0usize;
    for l in &lines { line_offsets.push(off); off += l.len() + 1; }

    // Track which functions have already been reported in this block
    // (matches vendor's `processed` set — only report each function body once)
    let mut reported_funcs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") { continue; }
        // Skip the $: declaration line unless it contains a .then()/.catch() callback
        if t.starts_with("$:") && !t.contains(".then(") && !t.contains(".catch(") { continue; }

        let line_byte_start = line_offsets[idx];

        // Determine if this line is in an async context (line-level checks)
        let in_callback = is_in_async_callback(block, line_byte_start, &all_triggers);
        let after_await = has_await_on_prev_line(block, line_byte_start);
        let in_then_catch = async_callback_regions.iter()
            .any(|&(start, end)| line_byte_start >= start && line_byte_start < end);
        // Also check if any position within this line falls inside a then/catch region
        // (handles single-line patterns like `$: foo().then((x) => (var = x))`)
        let line_end = line_byte_start + line.len();
        let line_overlaps_then_catch = async_callback_regions.iter()
            .any(|&(start, end)| start < line_end && end > line_byte_start);
        let in_async_ctx = in_callback || after_await || in_then_catch;

        if in_async_ctx {
            // Report direct assignments to tracked reactive vars.
            // Only flag variables that are actual dependencies of this $: block.
            let needs_ref_check = in_callback && !after_await && !in_then_catch;
            for var in top_vars {
                if local_names.contains(var) { continue; }
                // Only flag variables that are tracked dependencies of this block
                if !tracked_vars.contains(var.as_str()) { continue; }
                if !has_assign(t, var) { continue; }
                if needs_ref_check {
                    // For timer callbacks, check if the variable is read elsewhere
                    // in the block (outside this assignment). Simple assignments
                    // like `page = 1` where `page` is only written don't re-trigger.
                    // But member assignments like `obj.a = 1` DO trigger invalidation.
                    let is_member_assign = {
                        let dot = format!("{}.", var);
                        let bracket = format!("{}[", var);
                        t.contains(&dot) || t.contains(&bracket)
                    };
                    if !is_member_assign && !block_reads_var(block, var) { continue; }
                }

                let indent = line.len() - t.len();
                let abs = base + block_start + line_offsets[idx] + indent;
                ctx.diagnostic(
                    format!("Possibly it may occur an infinite reactive loop because `{}` is updated in an async callback.", var),
                    Span::new(abs as u32, abs as u32 + 1),
                );
            }

            // Report indirect function calls (with assignment sites)
            for fi in func_info {
                if fi.assigns.is_empty() { continue; }
                if local_names.contains(&fi.name) { continue; }
                if reported_funcs.contains(&fi.name) { continue; }
                let call_pat = format!("{}(", fi.name);
                if !t.contains(&call_pat) { continue; }

                for av in &fi.assigns {
                    if tracked_vars.contains(av.as_str()) && block_has_var_ref(block, av) {
                        let indent = line.len() - t.len();
                        let call_col = t.find(&call_pat).unwrap_or(0);
                        let abs = base + block_start + line_offsets[idx] + indent + call_col;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because this function may update `{}`.", av),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                        // Report at post-await assignment sites inside the function.
                        for (pos_var, pos_offset) in &fi.assign_positions_after_await {
                            if pos_var == av {
                                let abs = base + pos_offset;
                                ctx.diagnostic(
                                    "Possibly it may occur an infinite reactive loop.".to_string(),
                                    Span::new(abs as u32, abs as u32 + 1),
                                );
                            }
                        }
                        // When called after an await in the reactive block, also
                        // report at pre-await assignment sites by scanning the
                        // function body in the script content.
                        if after_await {
                            let (body_start, body_end) = fi.body_range;
                            if body_end <= ctx.source.len().saturating_sub(base) {
                                let body = &ctx.source[base + body_start..base + body_end];
                                let assign_pat = format!("{} = ", av);
                                for (pos, _) in body.match_indices(&assign_pat) {
                                    if is_word_start(body, pos) {
                                        let after_eq = pos + assign_pat.len();
                                        if after_eq < body.len() && body.as_bytes()[after_eq] == b'=' { continue; }
                                        let abs_pos = base + body_start + pos;
                                        ctx.diagnostic(
                                            "Possibly it may occur an infinite reactive loop.".to_string(),
                                            Span::new(abs_pos as u32, abs_pos as u32 + 1),
                                        );
                                    }
                                }
                            }
                        }
                        reported_funcs.insert(fi.name.clone());
                        break;
                    }
                }
            }
        }

        // Check for calls to async functions that assign reactive vars after await
        if !in_async_ctx {
            for fi in func_info {
                if !fi.has_await { continue; }
                if fi.assigns_after_await.is_empty() { continue; }
                if local_names.contains(&fi.name) { continue; }
                // Skip if this function has already been reported in this block
                // (matches vendor's processed set — each function body is only
                // analyzed once per reactive statement)
                if reported_funcs.contains(&fi.name) { continue; }
                let call_pat = format!("{}(", fi.name);
                if !t.contains(&call_pat) { continue; }
                // Word boundary check
                if let Some(cp) = t.find(&call_pat) {
                    if cp > 0 {
                        let prev = t.as_bytes()[cp - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' { continue; }
                    }
                }

                // Report at call site AND assignment sites inside the function.
                // ESLint reports one call-site diagnostic per assignment location.
                for (pos_var, pos_offset) in &fi.assign_positions_after_await {
                    if tracked_vars.contains(pos_var.as_str()) && block_has_var_ref(block, pos_var) {
                        // Report at call site in the reactive block
                        let indent = line.len() - t.len();
                        let call_col = t.find(&call_pat).unwrap_or(0);
                        let abs = base + block_start + line_offsets[idx] + indent + call_col;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because this function may update `{}`.", pos_var),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                        // Report at assignment site
                        let abs = base + pos_offset;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because `{}` is updated here.", pos_var),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                        // Report at intermediate call sites within the function body.
                        // When the assignment was propagated from a callee, report
                        // at the callee's call site inside this function's body.
                        let (body_start, body_end) = fi.body_range;
                        if body_end <= ctx.source.len().saturating_sub(base) {
                            let body = &ctx.source[base + body_start..base + body_end];
                            for callee_name in &fi.calls_after_await {
                                // Check if this callee assigns the flagged variable
                                let callee_fi = func_info.iter().find(|cf| cf.name == *callee_name);
                                if let Some(cf) = callee_fi {
                                    if cf.assigns.contains(pos_var) {
                                        let callee_call = format!("{}(", callee_name);
                                        if let Some(cpos) = body.find(&callee_call) {
                                            if is_word_start(body, cpos) {
                                                let abs_pos = base + body_start + cpos;
                                                ctx.diagnostic(
                                                    format!("Possibly it may occur an infinite reactive loop because this function may update `{}`.", pos_var),
                                                    Span::new(abs_pos as u32, abs_pos as u32 + 1),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                reported_funcs.insert(fi.name.clone());
            }
        }

        // Check for assignments within .then()/.catch() regions on this line
        // (handles single-line patterns like `$: foo().then((x) => (var = x))`)
        if !in_async_ctx && line_overlaps_then_catch {
            for var in top_vars {
                if local_names.contains(var) { continue; }
                if !has_assign(t, var) { continue; }
                // Find the assignment position within the line and check if it's
                // inside a then/catch region
                if let Some(assign_pos) = find_assign_pos(t, var) {
                    let indent = line.len() - t.len();
                    let abs_pos_in_block = line_byte_start + indent + assign_pos;
                    let in_region = async_callback_regions.iter()
                        .any(|&(start, end)| abs_pos_in_block >= start && abs_pos_in_block < end);
                    if in_region {
                        let abs = base + block_start + abs_pos_in_block;
                        ctx.diagnostic(
                            "Possibly it may occur an infinite reactive loop.".to_string(),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                    }
                }
            }
        }

        // Special case: `await funcName()` on this line
        if t.contains("await ") {
            for fi in func_info {
                if fi.assigns.is_empty() { continue; }
                if local_names.contains(&fi.name) { continue; }
                let await_call = format!("await {}(", fi.name);
                if !t.contains(&await_call) { continue; }

                for av in &fi.assigns {
                    if block_reads_var(block, av) {
                        let indent = line.len() - t.len();
                        let call_col = t.find(&format!("{}(", fi.name)).unwrap_or(0);
                        let abs = base + block_start + line_offsets[idx] + indent + call_col;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because this function may update `{}`.", av),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                        break;
                    }
                }
            }

            // Same-line await + assignment patterns:
            // 1. `var op= await expr` (assignment receives await result)
            // 2. `(await expr, (var op= val))` (comma expression after await)
            if !in_async_ctx {
                for var in top_vars {
                    if local_names.contains(var) { continue; }
                    if !has_assign(t, var) { continue; }

                    if let Some(assign_pos) = find_assign_pos(t, var) {
                        if let Some(await_pos) = t.find("await ") {
                            // Pattern 1: `var op= await ...` -> assign_pos < await_pos
                            if assign_pos < await_pos {
                                let indent = line.len() - t.len();
                                let abs = base + block_start + line_offsets[idx] + indent + assign_pos;
                                ctx.diagnostic(
                                    format!("Possibly it may occur an infinite reactive loop because `{}` is updated across an await boundary.", var),
                                    Span::new(abs as u32, abs as u32 + 1),
                                );
                                continue;
                            }
                            // Pattern 2: assignment after await, but NOT inside await's argument parens
                            // Check paren depth: if the assignment is at the same or shallower
                            // paren depth as the await keyword
                            if is_after_await_same_line(t, await_pos, assign_pos) {
                                let indent = line.len() - t.len();
                                let abs = base + block_start + line_offsets[idx] + indent + assign_pos;
                                ctx.diagnostic(
                                    format!("Possibly it may occur an infinite reactive loop because `{}` is updated across an await boundary.", var),
                                    Span::new(abs as u32, abs as u32 + 1),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
