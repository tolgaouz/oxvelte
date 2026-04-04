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

        let (mut top_vars, mut implicit_reactive) = collect_top_level_vars(content);
        if let Some(module) = &ctx.ast.module {
            let (mut module_vars, module_implicit) = collect_top_level_vars(&module.content);
            top_vars.append(&mut module_vars);
            implicit_reactive.extend(module_implicit);
        }
        let aliases = collect_store_refs_and_aliases(content, &mut top_vars);
        let func_info = collect_func_info(content, &top_vars, &implicit_reactive);

        let mut search_pos = 0;
        while let Some((bs, be)) = find_reactive_block(content, search_pos) {
            let block = &content[bs..be];
            analyze_block(ctx, block, bs, content_offset, &top_vars, &aliases, &func_info);
            search_pos = be;
        }
    }
}

fn collect_top_level_vars(content: &str) -> (Vec<String>, std::collections::HashSet<String>) {
    let mut vars = Vec::new();
    let mut implicit_reactive = std::collections::HashSet::new();
    for line in content.lines() {
        let t = line.trim();
        let t = t.strip_prefix("export ").unwrap_or(t);
        for kw in &["let ", "var ", "const "] {
            if let Some(rest) = t.strip_prefix(kw) {
                extract_declared_names(rest, &mut vars);
            }
        }
        if let Some(after) = t.strip_prefix("$:") {
            let after = after.trim_start();
            if let Some(eq_pos) = after.find('=') {
                let name = after[..eq_pos].trim();
                let post_eq = &after[eq_pos + 1..];
                if !name.is_empty()
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                    && !matches!(name.as_bytes()[0], b'{' | b'[')
                    && !matches!(post_eq.as_bytes().first(), Some(b'=' | b'>'))
                    && !vars.contains(&name.to_string())
                {
                    vars.push(name.to_string());
                    implicit_reactive.insert(name.to_string());
                }
            }
        }
    }
    (vars, implicit_reactive)
}

fn extract_declared_names(decl: &str, vars: &mut Vec<String>) {
    let trimmed = decl.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let close = if trimmed.starts_with('{') { '}' } else { ']' };
        if let Some(end) = trimmed.find(close) {
            let inner = &trimmed[1..end];
            for part in inner.split(',') {
                let p = part.trim();
                let name_part = if let Some(colon) = p.find(':') {
                    p[colon + 1..].trim()
                } else {
                    p
                };
                let name_part = if let Some(eq) = name_part.find('=') {
                    name_part[..eq].trim()
                } else {
                    name_part
                };
                let name_part = name_part.strip_prefix("...").unwrap_or(name_part);
                let name: String = name_part.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() { vars.push(name); }
            }
        }
    } else {
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

fn collect_store_refs_and_aliases(content: &str, vars: &mut Vec<String>) -> Vec<(String, String)> {
    const FNS: &[&str] = &["setTimeout", "setInterval", "queueMicrotask", "tick"];
    let mut aliases = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("const ") {
            if let Some(eq) = rest.find(" = ") {
                let val = rest[eq+3..].trim().trim_end_matches(';');
                if FNS.contains(&val) { aliases.push((val.to_string(), rest[..eq].to_string())); }
            }
        }
        if t.starts_with("import ") {
            if let (Some(bs), Some(be)) = (t.find('{'), t.find('}')) {
                for imp in t[bs+1..be].split(',') {
                    let imp = imp.trim();
                    let (orig, local) = match imp.find(" as ") {
                        Some(ap) => (imp[..ap].trim(), imp[ap+4..].trim()),
                        None => (imp, imp),
                    };
                    if !local.is_empty() {
                        let sr = format!("${}", local);
                        if content.contains(&sr) && !vars.contains(&sr) { vars.push(sr); }
                    }
                    if orig != local && FNS.contains(&orig) {
                        aliases.push((orig.to_string(), local.to_string()));
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
    assign_positions_after_await: Vec<(String, usize)>,
    all_assign_positions: Vec<(String, usize)>,
    calls_after_await: Vec<String>,
    all_calls: Vec<String>,
    body_range: (usize, usize),
    has_then_catch_assigns: bool,
}

fn merge_var(vec: &mut Vec<String>, val: &str) {
    if !vec.iter().any(|v| v == val) { vec.push(val.to_string()); }
}

fn merge_pos(vec: &mut Vec<(String, usize)>, var: &str, pos: usize, body_range: Option<(usize, usize)>) {
    if let Some((bs, be)) = body_range { if pos >= bs && pos < be { return; } }
    if !vec.iter().any(|(v, p)| v == var && *p == pos) { vec.push((var.to_string(), pos)); }
}

fn collect_func_info(content: &str, top_vars: &[String], implicit_reactive: &std::collections::HashSet<String>) -> Vec<FuncInfo> {
    let mut results = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

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

    let func_names_seen: Vec<String> = lines.iter().enumerate()
        .filter(|(idx, _)| line_depths[*idx] == 0)
        .filter_map(|(_, line)| extract_func_name(line.trim()))
        .collect();

    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        if line_depths[i] > 0 { i += 1; continue; }
        if let Some(name) = extract_func_name(t) {
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

                if j == i { continue; }

                if t.contains("await ") {
                    if seen_await || j > i {
                        for var in top_vars {
                            if var == &name { continue; }
                            if !t.contains(var.as_str()) { continue; }
                            if let Some(_) = find_var_op(t, var, " = await ") {
                                if is_local_declaration(t, var) { continue; }
                                if !assigns_after_await.contains(var) { assigns_after_await.push(var.clone()); }
                                assign_positions_after_await.push((var.clone(), line_pos + indent));
                                if !assigns.contains(var) { assigns.push(var.clone()); }
                                all_assign_positions.push((var.clone(), line_pos + indent));
                            }
                        }
                    }
                    seen_await = true;
                }

                for var in top_vars {
                    if var == &name { continue; }
                    if !t.contains(var.as_str()) { continue; }
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

                for other in &func_names_seen {
                    if other == &name { continue; }
                    let call_pat = format!("{}(", other);
                    if let Some(cp) = t.find(&call_pat) {
                        if is_word_start(t, cp) {
                            if !all_calls.contains(other) { all_calls.push(other.clone()); }
                            if seen_await && !calls_after_await.contains(other) {
                                calls_after_await.push(other.clone());
                            }
                        }
                    }
                }
            }

            let body_start = func_start_offset;
            let body_end = if end_line < line_offsets.len() { line_offsets[end_line] + lines[end_line].len() } else { content.len() };

            let func_body = &content[body_start..body_end];
            let then_catch_regions = find_callback_regions(func_body, &[".then(", ".catch("], false, false);
            let timer_regions = find_callback_regions(func_body, &["setTimeout(", "setInterval(", "queueMicrotask("], true, true);
            let mut has_then_catch_assigns = false;
            let mut all_async_regions = then_catch_regions;
            all_async_regions.extend_from_slice(&timer_regions);
            if !all_async_regions.is_empty() {
                for j in i+1..=end_line {
                    let line = lines[j];
                    let lt = line.trim();
                    let indent = line.len() - lt.len();
                    let lpos = line_offsets[j] - body_start; // position relative to func body
                    for var in top_vars {
                        if implicit_reactive.contains(var) { continue; }
                        if !lt.contains(var.as_str()) { continue; }
                        if is_local_declaration(lt, var) { continue; }
                        if !has_assign(lt, var) { continue; }
                        let in_region = all_async_regions.iter()
                            .any(|&(start, end)| lpos + indent >= start && lpos + indent < end);
                        if in_region {
                            has_then_catch_assigns = true;
                            if !assigns_after_await.contains(var) {
                                assigns_after_await.push(var.clone());
                            }
                            assign_positions_after_await.push((var.clone(), line_offsets[j] + indent));
                        }
                    }
                }
            }

            results.push(FuncInfo { name, assigns, has_await, assigns_after_await, assign_positions_after_await, all_assign_positions, calls_after_await, all_calls, body_range: (body_start, body_end), has_then_catch_assigns });
        }
        i += 1;
    }

    for _ in 0..4 {
        let snapshot: Vec<(String, Vec<String>, Vec<(String, usize)>, Vec<String>, Vec<(String, usize)>, bool, bool)> = results.iter()
            .map(|fi| (fi.name.clone(), fi.assigns.clone(), fi.all_assign_positions.clone(), fi.assigns_after_await.clone(), fi.assign_positions_after_await.clone(), fi.has_await, fi.has_then_catch_assigns))
            .collect();

        for fi in results.iter_mut() {
            let br = fi.body_range;
            for callee_name in fi.calls_after_await.clone() {
                if let Some((_, ca, cp, caa, caap, cha, _)) = snapshot.iter().find(|(n, ..)| n == &callee_name) {
                    let (vi, pi) = if *cha { (caa, caap) } else { (ca, cp) };
                    for v in vi { merge_var(&mut fi.assigns_after_await, v); }
                    for v in ca { merge_var(&mut fi.assigns, v); }
                    for (v, p) in pi { merge_pos(&mut fi.assign_positions_after_await, v, *p, Some(br)); }
                    for (v, p) in cp { merge_pos(&mut fi.all_assign_positions, v, *p, None); }
                }
            }
            for callee_name in fi.all_calls.clone() {
                if let Some((_, ca, cp, caa, caap, cha, chtc)) = snapshot.iter().find(|(n, ..)| n == &callee_name) {
                    for v in ca { merge_var(&mut fi.assigns, v); }
                    for (v, p) in cp { merge_pos(&mut fi.all_assign_positions, v, *p, None); }
                    if !caa.is_empty() {
                        for v in caa { merge_var(&mut fi.assigns_after_await, v); }
                        for (v, p) in caap { merge_pos(&mut fi.assign_positions_after_await, v, *p, Some(br)); }
                        fi.has_await |= cha;
                        fi.has_then_catch_assigns |= chtc;
                    }
                }
            }
        }
    }

    results
}

fn extract_func_name(line: &str) -> Option<String> {
    for prefix in &["const ", "let "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            if let Some(eq) = rest.find(" = ") {
                if is_direct_function_expr(rest[eq+3..].trim()) {
                    return Some(rest[..eq].to_string());
                }
            }
        }
    }
    let rest = line.strip_prefix("function ")
        .or_else(|| line.strip_prefix("async function "))?;
    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if !name.is_empty() { Some(name) } else { None }
}

fn is_direct_function_expr(after: &str) -> bool {
    if after.starts_with("function") || after.starts_with("async") {
        return true;
    }
    if after.starts_with('(') {
        let mut depth = 0i32;
        for (i, ch) in after.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let rest = after[i + 1..].trim_start();
                        if rest.starts_with("=>") { return true; }
                        if rest.starts_with(':') && rest.contains("=>") {
                            return true;
                        }
                        return false;
                    }
                }
                _ => {}
            }
        }
        return false;
    }
    let ident_end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
    ident_end > 0 && after[ident_end..].trim_start().starts_with("=>")
}

fn is_local_declaration(line: &str, var: &str) -> bool {
    for kw in &["const ", "let ", "var "] {
        let Some(kw_pos) = line.find(kw) else { continue };
        let after_kw = &line[kw_pos + kw.len()..];
        if matches!(after_kw.as_bytes().first(), Some(b'{' | b'[')) {
            if let Some(close) = after_kw.find(|c: char| c == '}' || c == ']') {
                if after_kw[1..close].split(',').any(|p| p.trim() == var) { return true; }
            }
        } else {
            let end = after_kw.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                .unwrap_or(after_kw.len());
            if &after_kw[..end] == var { return true; }
        }
    }
    false
}

fn has_assign(line: &str, var: &str) -> bool {
    if !line.contains(var) { return false; }
    let ops = [" = ", " += ", " -= ", " *= ", " /= "];
    for op in &ops {
        if let Some(pos) = find_var_op(line, var, op) {
            if *op == " = " && pos + var.len() + op.len() < line.len()
                && line.as_bytes()[pos + var.len() + op.len()] == b'=' { continue; }
            return true;
        }
    }
    if let Some(pos) = find_var_op(line, var, " =") {
        let after = pos + var.len() + 2;
        if after >= line.len() || !matches!(line.as_bytes()[after], b'=' | b'>') { return true; }
    }
    has_member_assign(line, var, &ops)
}

fn find_var_op(line: &str, var: &str, op: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = line[start..].find(var) {
        let abs = start + pos;
        if is_word_start(line, abs) {
            let after = abs + var.len();
            if line[after..].starts_with(op) {
                let before = line[..abs].trim_end();
                if before.ends_with("typeof") {
                    start = abs + 1;
                    continue;
                }
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
    !(b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'.')
}

fn find_assign_pos(line: &str, var: &str) -> Option<usize> {
    for op in &[" = ", " += ", " -= "] {
        if let Some(pos) = find_var_op(line, var, op) {
            if *op == " = " {
                let a = pos + var.len() + 3;
                if a < line.len() && line.as_bytes()[a] == b'=' { continue; }
            }
            return Some(pos);
        }
    }
    let mut start = 0;
    while let Some(p) = line[start..].find(var) {
        let pos = start + p;
        if is_word_start(line, pos) && line[pos + var.len()..].starts_with('.') {
            let rest = &line[pos + var.len() + 1..];
            if [" = ", " += ", " -= "].iter().any(|op| rest.contains(op)) { return Some(pos); }
        }
        start = pos + 1;
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
    let bytes = content.as_bytes();
    let mut i = start;
    let mut depth = 0i32;
    while i < bytes.len() {
        if let Some(end) = skip_comment_raw(bytes, i) { i = end; continue; }
        match bytes[i] {
            b'\'' | b'"' | b'`' => { i = skip_string_raw(bytes, i); continue; }
            b'{' => depth += 1,
            b'}' => { depth -= 1; if depth == 0 { return i + 1; } }
            _ => {}
        }
        i += 1;
    }
    content.len()
}

fn find_stmt_end(content: &str, dollar: usize) -> usize {
    let mut pdepth = 0i32;
    let mut bdepth = 0i32;
    let mut has_c = false;
    let bytes = content.as_bytes();
    let mut i = dollar + 2;
    while i < bytes.len() {
        if let Some(end) = skip_comment_raw(bytes, i) {
            if bytes[end.min(bytes.len()) - 1] == b'\n' && pdepth <= 0 && bdepth <= 0 && has_c {
                // line comment ended with newline — check newline logic below
            }
            i = end;
            continue;
        }
        let b = bytes[i];
        match b {
            b'\'' | b'"' | b'`' => { has_c = true; i = skip_string_raw(bytes, i); continue; }
            b'{' => { bdepth += 1; has_c = true; }
            b'}' => {
                bdepth -= 1;
                if bdepth <= 0 && pdepth <= 0 && has_c {
                    let rest = content[i + 1..].trim_start();
                    if !rest.starts_with("else") { return i + 1; }
                }
            }
            b'(' => { pdepth += 1; has_c = true; }
            b')' => {
                pdepth -= 1;
                if pdepth <= 0 && bdepth <= 0 && has_c {
                    let rest = &content[i + 1..];
                    let nt = rest.trim_start_matches(|c: char| c == ' ' || c == '\t');
                    if nt.starts_with(';') {
                        return i + 1 + (rest.len() - nt.len()) + 1;
                    }
                    if nt.starts_with('\n') || nt.is_empty() {
                        let after_ws = nt.trim_start();
                        let is_continuation = after_ws.as_bytes().first().is_some_and(|b|
                            matches!(b, b'.' | b'?' | b':' | b'+' | b'-' | b'*' | b'/'
                                | b'%' | b'&' | b'|' | b'^' | b'<' | b'>' | b'=' | b'!' | b',' | b'('));
                        if !is_continuation { return i + 1 + (rest.len() - nt.len()); }
                    }
                }
            }
            b';' if pdepth <= 0 && bdepth <= 0 && has_c => return i + 1,
            b'\n' if pdepth <= 0 && bdepth <= 0 && has_c => {
                let rest = &content[i + 1..];
                let next_line = rest.trim_start();
                if next_line.is_empty() || rest.starts_with('\n')
                    || (rest.starts_with("\r\n") && !next_line.is_empty()) { return i + 1; }
                const STMT_STARTS: &[&str] = &["async ", "function ", "const ", "let ", "var ",
                    "if ", "if(", "for ", "for(", "while ", "while(", "return ",
                    "return;", "throw ", "class ", "import ", "export ",
                    "try ", "try{", "switch ", "switch(", "$: "];
                if STMT_STARTS.iter().any(|kw| next_line.starts_with(kw)) { return i + 1; }
            }
            _ if !b.is_ascii_whitespace() => has_c = true,
            _ => {}
        }
        i += 1;
    }
    content.len()
}

fn block_has_var_ref(block: &str, var: &str) -> bool {
    block.match_indices(var).any(|(pos, _)| {
        is_word_start(block, pos) && {
            let a = pos + var.len();
            a >= block.len() || !matches!(block.as_bytes()[a], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$')
        }
    })
}

fn block_reads_var(block: &str, var: &str) -> bool {
    for line in block.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("$:") { continue; }
        if !t.contains(var) { continue; }

        let count = t.match_indices(var)
            .filter(|(pos, _)| is_word_start(t, *pos))
            .count();
        if count == 0 { continue; }

        if count > 1 { return true; }

        let simple_assign = find_var_op(t, var, " = ").is_some_and(|pos| {
            let a = pos + var.len() + 3;
            a >= t.len() || t.as_bytes()[a] != b'='
        });
        let member_assign = has_member_assign(t, var, &[" = ", " += ", " -= ", " *= ", " /= "]);
        if simple_assign || member_assign {
            continue; // write-only
        }

        return true;
    }
    false
}

fn is_in_async_callback(block: &str, pos: usize, all_triggers: &[String]) -> bool {
    let before = &block[..pos.min(block.len())];
    let mut pd = 0i32;
    let bytes = before.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => pd += 1,
            b'(' if pd > 0 => pd -= 1,
            b'(' => {
                let bp = before[..i].trim_end();
                for trigger in all_triggers {
                    if bp.ends_with(trigger.as_str()) {
                        let s = bp.len() - trigger.len();
                        if s == 0 || !matches!(bp.as_bytes()[s-1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn has_await_on_prev_line(block: &str, line_start_pos: usize) -> bool {
    let before = &block[..line_start_pos.min(block.len())];
    if !before.contains("async ") && !before.contains("async(") { return false; }
    let bytes = before.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut sch = b'"';
    let mut async_body_depth = 0i32;
    let mut await_positions = Vec::new();
    for i in 0..bytes.len() {
        if in_str {
            if bytes[i] == sch && (i == 0 || bytes[i-1] != b'\\') { in_str = false; }
            continue;
        }
        match bytes[i] {
            b'\'' | b'"' | b'`' => { in_str = true; sch = bytes[i]; }
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if i + 6 <= bytes.len() && &before[i..i+6] == "async " {
            async_body_depth = depth + 1;
        }
        if depth >= async_body_depth && i + 6 <= bytes.len() && &before[i..i+6] == "await "
            && (i == 0 || !bytes[i-1].is_ascii_alphanumeric()) {
            await_positions.push(depth);
        }
    }
    await_positions.iter().any(|&d| d <= depth)
}

fn skip_string_raw(bytes: &[u8], start: usize) -> usize {
    let q = bytes[start];
    let mut j = start + 1;
    while j < bytes.len() {
        if bytes[j] == b'\\' { j += 2; continue; }
        if bytes[j] == q { return j + 1; }
        j += 1;
    }
    j
}

fn skip_comment_raw(bytes: &[u8], i: usize) -> Option<usize> {
    if i + 1 >= bytes.len() || bytes[i] != b'/' { return None; }
    if bytes[i + 1] == b'/' {
        let mut j = i + 2;
        while j < bytes.len() && bytes[j] != b'\n' { j += 1; }
        return Some(j);
    }
    if bytes[i + 1] == b'*' {
        let mut j = i + 2;
        while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') { j += 1; }
        return Some(if j + 1 < bytes.len() { j + 2 } else { j });
    }
    None
}

fn skip_brace_body(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1i32;
    while i < bytes.len() && depth > 0 {
        if let Some(end) = skip_comment_raw(bytes, i) { i = end; continue; }
        match bytes[i] {
            b'\'' | b'"' | b'`' => { i = skip_string_raw(bytes, i); continue; }
            b'{' => depth += 1,
            b'}' => { depth -= 1; if depth == 0 { return i; } }
            _ => {}
        }
        i += 1;
    }
    i
}

fn scan_callback_args(bytes: &[u8], after: usize, first_only: bool) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut i = after;
    let mut paren_depth = 1i32;
    while i < bytes.len() && paren_depth > 0 {
        if let Some(end) = skip_comment_raw(bytes, i) { i = end; continue; }
        match bytes[i] {
            b'\'' | b'"' | b'`' => { i = skip_string_raw(bytes, i); }
            b'(' => { paren_depth += 1; i += 1; }
            b')' => { paren_depth -= 1; i += 1; }
            b'=' if i + 1 < bytes.len() && bytes[i + 1] == b'>' && paren_depth == 1 => {
                i += 2;
                while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') { i += 1; }
                if i < bytes.len() && bytes[i] == b'{' {
                    let body_start = i;
                    i = skip_brace_body(bytes, i);
                    regions.push((body_start, i));
                    if i < bytes.len() { i += 1; }
                } else if i < bytes.len() {
                    let body_start = i;
                    let mut pd = 0i32;
                    while i < bytes.len() {
                        if let Some(end) = skip_comment_raw(bytes, i) { i = end; continue; }
                        match bytes[i] {
                            b'\'' | b'"' | b'`' => { i = skip_string_raw(bytes, i); continue; }
                            b'(' => { pd += 1; i += 1; }
                            b')' if pd > 0 => { pd -= 1; i += 1; }
                            b')' | b',' if pd == 0 => break,
                            _ => { i += 1; }
                        }
                    }
                    regions.push((body_start, i));
                }
                if first_only { break; }
            }
            b'{' if paren_depth == 1 => {
                let body_start = i;
                i = skip_brace_body(bytes, i);
                regions.push((body_start, i));
                if i < bytes.len() { i += 1; }
                if first_only { break; }
            }
            _ => { i += 1; }
        }
    }
    regions
}

fn find_callback_regions(block: &str, patterns: &[&str], check_boundary: bool, first_only: bool) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let bytes = block.as_bytes();
    for pattern in patterns {
        let mut search = 0;
        while let Some(pos) = block[search..].find(pattern) {
            let abs = search + pos;
            if check_boundary && abs > 0
                && matches!(bytes[abs - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$')
            {
                search = abs + pattern.len(); continue;
            }
            regions.extend(scan_callback_args(bytes, abs + pattern.len(), first_only));
            search = abs + pattern.len();
        }
    }
    regions
}

fn is_in_effective_then_catch(regions: &[(usize, usize)], pos: usize) -> bool {
    let innermost = match regions.iter()
        .filter(|(s, e)| pos >= *s && pos < *e)
        .min_by_key(|(s, e)| e - s) {
        Some(r) => r,
        None => return false,
    };
    !regions.iter().any(|(s, e)| *s > innermost.0 && *e < innermost.1 && *e <= pos)
}

fn report_intermediate_calls(
    ctx: &mut LintContext,
    fi: &FuncInfo,
    func_info: &[FuncInfo],
    pos_var: &str,
    base: usize,
) {
    let (body_start, body_end) = fi.body_range;
    if body_end > ctx.source.len().saturating_sub(base) { return; }
    let body = &ctx.source[base + body_start..base + body_end];

    let mut reported = std::collections::HashSet::new();
    let mut stack: Vec<&str> = Vec::new();
    for callee_name in fi.all_calls.iter().chain(fi.calls_after_await.iter()) {
        if !stack.contains(&callee_name.as_str()) {
            stack.push(callee_name.as_str());
        }
    }

    while let Some(callee_name) = stack.pop() {
        if reported.contains(callee_name) { continue; }
        let callee_fi = match func_info.iter().find(|cf| cf.name == callee_name) {
            Some(cf) => cf,
            None => continue,
        };
        if !callee_fi.assigns.contains(&pos_var.to_string()) { continue; }

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
        reported.insert(callee_name);

        let (cb_start, cb_end) = callee_fi.body_range;
        if cb_end <= ctx.source.len().saturating_sub(base) {
            let callee_body = &ctx.source[base + cb_start..base + cb_end];
            for deeper_callee in callee_fi.all_calls.iter() {
                if reported.contains(deeper_callee.as_str()) { continue; }
                let deeper_fi = match func_info.iter().find(|cf| cf.name == *deeper_callee) {
                    Some(cf) => cf,
                    None => continue,
                };
                if !deeper_fi.assigns.contains(&pos_var.to_string()) { continue; }
                let deeper_call = format!("{}(", deeper_callee);
                if let Some(dpos) = callee_body.find(&deeper_call) {
                    if is_word_start(callee_body, dpos) {
                        let abs_pos = base + cb_start + dpos;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because this function may update `{}`.", pos_var),
                            Span::new(abs_pos as u32, abs_pos as u32 + 1),
                        );
                    }
                }
                reported.insert(deeper_callee.as_str());
            }
        }
    }
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
    let mut all_triggers: Vec<String> = ["setTimeout", "setInterval", "queueMicrotask", "tick"]
        .iter().map(|s| s.to_string()).collect();
    for (_, alias) in aliases { all_triggers.push(alias.clone()); }


    let local_names: Vec<String> = block.lines()
        .filter_map(|line| {
            let t = line.trim();
            let rest = t.strip_prefix("let ").or_else(|| t.strip_prefix("const "))
                .or_else(|| t.strip_prefix("var ")).or_else(|| t.strip_prefix("function "))?;
            let name: String = rest.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !name.is_empty() { Some(name) } else { None }
        })
        .collect();

    let async_callback_regions = find_callback_regions(block, &[".then(", ".catch("], false, false);

    let tracked_vars: std::collections::HashSet<String> = {
        let mut tracked = std::collections::HashSet::new();
        let bytes = block.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\'' | b'"' | b'`' => { i = skip_string_raw(bytes, i); continue; }
                b'/' => if let Some(end) = skip_comment_raw(bytes, i) { i = end; continue; },
                b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
                    if i > 0 && bytes[i-1] == b'.' && !(i >= 3 && bytes[i-2] == b'.' && bytes[i-3] == b'.') {
                        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                        continue;
                    }
                    let start = i;
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                    let ident = &block[start..i];
                    let after_ident = block[i..].trim_start();
                    if after_ident.starts_with(':') && !after_ident.starts_with("::") {
                        let before_start = block[..start].trim_end();
                        if before_start.ends_with('{') || before_start.ends_with(',')
                            || before_start.ends_with('\n') { continue; }
                    }
                    if top_vars.iter().any(|v| v == ident) && !local_names.contains(&ident.to_string()) {
                        tracked.insert(ident.to_string());
                    }
                }
                _ => { i += 1; }
            }
        }
        tracked
    };

    let lines: Vec<&str> = block.lines().collect();
    let mut line_offsets = Vec::new();
    let mut off = 0usize;
    for l in &lines { line_offsets.push(off); off += l.len() + 1; }

    let mut reported_funcs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") { continue; }
        let is_dollar_line = t.starts_with("$:");

        let line_byte_start = line_offsets[idx];

        let in_callback = is_in_async_callback(block, line_byte_start, &all_triggers);
        let after_await = has_await_on_prev_line(block, line_byte_start);
        let in_then_catch = is_in_effective_then_catch(&async_callback_regions, line_byte_start);
        let line_end = line_byte_start + line.len();
        let line_overlaps_then_catch = async_callback_regions.iter()
            .any(|&(start, end)| start < line_end && end > line_byte_start);
        let in_async_ctx = in_callback || after_await || in_then_catch;

        if in_async_ctx && !is_dollar_line {
            for var in top_vars {
                if local_names.contains(var) { continue; }
                if !tracked_vars.contains(var.as_str()) { continue; }
                if !has_assign(t, var) { continue; }

                let indent = line.len() - t.len();
                let abs = base + block_start + line_offsets[idx] + indent;
                ctx.diagnostic(
                    format!("Possibly it may occur an infinite reactive loop because `{}` is updated in an async callback.", var),
                    Span::new(abs as u32, abs as u32 + 1),
                );
            }

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
                        for (pos_var, pos_offset) in &fi.all_assign_positions {
                            if pos_var == av {
                                let abs = base + pos_offset;
                                ctx.diagnostic(
                                    "Possibly it may occur an infinite reactive loop.".to_string(),
                                    Span::new(abs as u32, abs as u32 + 1),
                                );
                            }
                        }
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

        if !in_async_ctx {
            for fi in func_info {
                if !fi.has_await && !fi.has_then_catch_assigns { continue; }
                if fi.assigns_after_await.is_empty() { continue; }
                if local_names.contains(&fi.name) { continue; }
                if reported_funcs.contains(&fi.name) { continue; }
                let call_pat = format!("{}(", fi.name);
                if !t.contains(&call_pat) { continue; }
                if let Some(cp) = t.find(&call_pat) {
                    if cp > 0 {
                        let prev = t.as_bytes()[cp - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' { continue; }
                    }
                }

                for (pos_var, pos_offset) in &fi.assign_positions_after_await {
                    if tracked_vars.contains(pos_var.as_str()) && block_has_var_ref(block, pos_var) {
                        let indent = line.len() - t.len();
                        let call_col = t.find(&call_pat).unwrap_or(0);
                        let abs = base + block_start + line_offsets[idx] + indent + call_col;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because this function may update `{}`.", pos_var),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                        let abs = base + pos_offset;
                        ctx.diagnostic(
                            format!("Possibly it may occur an infinite reactive loop because `{}` is updated here.", pos_var),
                            Span::new(abs as u32, abs as u32 + 1),
                        );
                        report_intermediate_calls(ctx, fi, func_info, pos_var, base);
                    }
                }
                reported_funcs.insert(fi.name.clone());
            }
        }

        if !in_async_ctx && line_overlaps_then_catch {
            for var in top_vars {
                if local_names.contains(var) { continue; }
                if !has_assign(t, var) { continue; }
                if let Some(assign_pos) = find_assign_pos(t, var) {
                    let indent = line.len() - t.len();
                    let abs_pos_in_block = line_byte_start + indent + assign_pos;
                    let in_region = is_in_effective_then_catch(&async_callback_regions, abs_pos_in_block);
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

            if !in_async_ctx {
                for var in top_vars {
                    if local_names.contains(var) { continue; }
                    if !has_assign(t, var) { continue; }

                    if let Some(assign_pos) = find_assign_pos(t, var) {
                        if let Some(await_pos) = t.find("await ") {
                            let across = assign_pos < await_pos
                                || (assign_pos > await_pos && t[await_pos..assign_pos].contains(','));
                            if across {
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
