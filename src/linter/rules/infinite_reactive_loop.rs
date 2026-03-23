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
        for kw in &["let ", "var "] {
            if let Some(rest) = t.strip_prefix(kw) {
                let name: String = rest.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() { vars.push(name); }
            }
        }
    }
    vars
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
    assign_positions_after_await: Vec<(String, usize)>, // (var_name, content_offset)
}

fn collect_func_info(content: &str, top_vars: &[String]) -> Vec<FuncInfo> {
    let mut results = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Pre-compute brace depth at the start of each line (to filter out nested functions)
    let mut line_depths = Vec::with_capacity(lines.len());
    let mut depth = 0i32;
    for &line in &lines {
        line_depths.push(depth);
        for ch in line.chars() {
            match ch { '{' => depth += 1, '}' => depth -= 1, _ => {} }
        }
    }

    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        // Only collect top-level functions (brace depth 0)
        if line_depths[i] > 0 { i += 1; continue; }
        if let Some(name) = extract_func_name(t) {
            let mut depth = 0i32;
            let mut body = String::new();
            let mut started = false;
            for j in i..lines.len() {
                for ch in lines[j].chars() {
                    if ch == '{' { depth += 1; started = true; }
                    if ch == '}' { depth -= 1; }
                }
                body.push_str(lines[j]);
                body.push('\n');
                if started && depth <= 0 { break; }
            }
            let assigns: Vec<String> = top_vars.iter()
                .filter(|v| body.lines().any(|l| has_assign(l.trim(), v)))
                .cloned().collect();

            // Check if function has await and which vars are assigned after it
            let has_await = body.contains("await ");
            let (assigns_after_await, assign_positions_after_await) = if has_await {
                // Calculate the content offset where this function body starts
                let func_content_offset: usize = lines[..i].iter().map(|l| l.len() + 1).sum();
                collect_assigns_after_await_with_positions(&body, top_vars, func_content_offset)
            } else {
                (Vec::new(), Vec::new())
            };

            results.push(FuncInfo { name, assigns, has_await, assigns_after_await, assign_positions_after_await });
        }
        i += 1;
    }
    results
}

/// Collect variables assigned after an `await` in a function body.
/// Also returns (var_name, content_offset) pairs for each assignment.
fn collect_assigns_after_await_with_positions(
    body: &str, top_vars: &[String], body_content_offset: usize,
) -> (Vec<String>, Vec<(String, usize)>) {
    let mut result = Vec::new();
    let mut positions = Vec::new();
    let mut seen_await = false;
    let mut line_offset = 0usize;
    for line in body.lines() {
        let t = line.trim();
        let indent = line.len() - t.len();
        if t.contains("await ") {
            for var in top_vars {
                let ops = [
                    format!("{} = await ", var),
                    format!("{} += await ", var),
                    format!("{} -= await ", var),
                ];
                if ops.iter().any(|pat| t.contains(pat.as_str())) {
                    if !result.contains(var) { result.push(var.clone()); }
                    positions.push((var.clone(), body_content_offset + line_offset + indent));
                }
            }
            seen_await = true;
            line_offset += line.len() + 1;
            continue;
        }
        if seen_await {
            for var in top_vars {
                if has_assign(t, var) {
                    if !result.contains(var) { result.push(var.clone()); }
                    positions.push((var.clone(), body_content_offset + line_offset + indent));
                }
            }
        }
        line_offset += line.len() + 1;
    }
    (result, positions)
}

fn extract_func_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("const ") {
        if let Some(eq) = rest.find(" = ") {
            let name = &rest[..eq];
            let after = rest[eq+3..].trim();
            if after.contains("=>") || after.starts_with("function")
                || after.starts_with("async") || after.starts_with("(") {
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
            if after.contains("=>") || after.starts_with("function")
                || after.starts_with("async") || after.starts_with("(") {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn has_assign(line: &str, var: &str) -> bool {
    let ops = [" = ", " += ", " -= ", " *= ", " /= "];
    for op in &ops {
        let pat = format!("{}{}", var, op);
        if let Some(pos) = line.find(&pat) {
            if !is_word_start(line, pos) { continue; }
            if *op == " = " && pos + pat.len() < line.len() && line.as_bytes()[pos + pat.len()] == b'=' { continue; }
            return true;
        }
        // Check property/index access: var.prop = , var[idx] = , var.prop[idx].etc =
        for prefix in &[format!("{}.", var), format!("{}[", var)] {
            for (pos, _) in line.match_indices(prefix.as_str()) {
                if !is_word_start(line, pos) { continue; }
                let rest = &line[pos + prefix.len()..];
                // Skip initial property name (after .) or index expression (after [)
                let mut r = if prefix.ends_with('.') {
                    let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
                    &rest[end..]
                } else {
                    // After [, find matching ]
                    rest.find(']').map(|p| &rest[p+1..]).unwrap_or("")
                };
                // Follow chained property/index access
                loop {
                    if r.starts_with('[') {
                        if let Some(close) = r.find(']') { r = &r[close+1..]; } else { break; }
                    } else if r.starts_with('.') {
                        let end = r[1..].find(|c: char| !c.is_alphanumeric() && c != '_').map(|e| e+1).unwrap_or(r.len());
                        r = &r[end..];
                    } else {
                        break;
                    }
                }
                let r = r.trim_start();
                for op2 in &ops {
                    if r.starts_with(op2.trim()) {
                        if *op2 == " = " && r.len() > 1 && r.as_bytes()[1] == b'=' { continue; }
                        return true;
                    }
                }
            }
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
                // If we close the outermost brace, check if followed by `else`
                // (if-else blocks continue past the first })
                if bdepth <= 0 && has_c {
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
                    if nt.starts_with(';') || nt.starts_with('\n') || nt.is_empty() {
                        return abs + 1 + (rest.len() - nt.len()) + if nt.starts_with(';') { 1 } else { 0 };
                    }
                }
            }
            ';' if pdepth <= 0 && bdepth <= 0 && has_c => return abs + 1,
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

    // Second pass: find `await` keywords at the same depth on preceding lines
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

        // Check for `await ` at the target depth
        if depth == target_depth && i + 6 <= before.len() && &before[i..i+6] == "await " {
            // Make sure it's a whole word (not part of another identifier)
            if i == 0 || !bytes[i-1].is_ascii_alphanumeric() {
                // Make sure this await is on a line BEFORE the current line
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
fn find_then_catch_regions(block: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let bytes = block.as_bytes();

    // Find all `.then(` and `.catch(` positions
    for pattern in &[".then(", ".catch("] {
        let mut search = 0;
        while let Some(pos) = block[search..].find(pattern) {
            let abs = search + pos;
            let after = abs + pattern.len();

            // Find the callback body: skip to the `{` of the arrow/function body
            // Track parens to find the callback argument
            let mut i = after;
            let mut paren_depth = 1i32;
            let mut body_start = None;
            let mut body_end = None;

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

    let lines: Vec<&str> = block.lines().collect();
    let mut line_offsets = Vec::new();
    let mut off = 0usize;
    for l in &lines { line_offsets.push(off); off += l.len() + 1; }

    for (idx, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("$:") { continue; }

        let line_byte_start = line_offsets[idx];

        // Determine if this line is in an async context
        let in_callback = is_in_async_callback(block, line_byte_start, &all_triggers);
        let after_await = has_await_on_prev_line(block, line_byte_start);
        let in_then_catch = async_callback_regions.iter()
            .any(|&(start, end)| line_byte_start >= start && line_byte_start < end);
        let in_async_ctx = in_callback || after_await || in_then_catch;

        if in_async_ctx {
            // Report direct assignments to reactive vars.
            // For callback-only context (setTimeout etc.), only flag vars the block reads
            // (assigning unrelated vars in setTimeout can't re-trigger the block).
            // For after-await and .then/.catch, flag all reactive var assignments.
            let needs_read_check = in_callback && !after_await && !in_then_catch;
            for var in top_vars {
                if local_names.contains(var) { continue; }
                if !has_assign(t, var) { continue; }
                if needs_read_check && !block_reads_var(block, var) { continue; }

                let indent = line.len() - t.len();
                let abs = base + block_start + line_offsets[idx] + indent;
                ctx.diagnostic(
                    "Possibly it may occur an infinite reactive loop.",
                    Span::new(abs as u32, abs as u32 + 1),
                );
            }

            // Report indirect function calls (with assignment sites)
            for fi in func_info {
                if fi.assigns.is_empty() { continue; }
                if local_names.contains(&fi.name) { continue; }
                let call_pat = format!("{}(", fi.name);
                if !t.contains(&call_pat) { continue; }

                for av in &fi.assigns {
                    if block_has_var_ref(block, av) {
                        let indent = line.len() - t.len();
                        let call_col = t.find(&call_pat).unwrap_or(0);
                        let abs = base + block_start + line_offsets[idx] + indent + call_col;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because this function may update `{}`.", av),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                        // Also report at assignment sites inside the function
                        for (pos_var, pos_offset) in &fi.assign_positions_after_await {
                            if pos_var == av {
                                let abs = base + pos_offset;
                                ctx.diagnostic(
                                    "Possibly it may occur an infinite reactive loop.",
                                    Span::new(abs as u32, abs as u32 + 1),
                                );
                            }
                        }
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
                    if block_has_var_ref(block, pos_var) {
                        // Report at call site
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
                            "Possibly it may occur an infinite reactive loop.",
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
                                    "Possibly it may occur an infinite reactive loop.",
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
                                    "Possibly it may occur an infinite reactive loop.",
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
