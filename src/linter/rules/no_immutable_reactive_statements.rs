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

        let mut immutable_names: HashSet<&str> = HashSet::new();
        let mut const_names: HashSet<&str> = HashSet::new();
        let mut let_names: HashSet<&str> = HashSet::new();
        let mut prop_names: HashSet<&str> = HashSet::new();

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
                    prop_names.insert(n);
                } else if trimmed.starts_with("let ") || trimmed.starts_with("var ") {
                    let_names.insert(n);
                } else if trimmed.starts_with("const ") || trimmed.starts_with("export const ") {
                    if !prop_names.contains(n) && !let_names.contains(n) {
                        const_names.insert(n);
                    }
                } else if ["function ", "export function ", "class ", "export class ",
                    "type ", "export type ", "interface ", "export interface ",
                    "enum ", "export enum "].iter().any(|p| trimmed.starts_with(p)) {
                    immutable_names.insert(n);
                } else if trimmed.starts_with("import ") {
                    for imp in extract_import_names(trimmed) {
                        immutable_names.insert(imp);
                    }
                }
            } else {
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

        extract_multiline_imports(content, &mut immutable_names);
        let reactive_stmts = collect_reactive_stmts(content);

        let is_ts = script.lang.as_deref() == Some("ts")
            || script.lang.as_deref() == Some("typescript");

        let mut mutable_lets: HashSet<&str> = HashSet::new();
        for &var in &let_names {
            if has_reassignment(content, var) || has_reassignment(ctx.source, var) {
                mutable_lets.insert(var);
            }
        }

        let mut const_member_written: HashSet<&str> = HashSet::new();
        for &var in &const_names {
            if has_member_write(content, var) || has_member_write(ctx.source, var) {
                const_member_written.insert(var);
            }
        }

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

        let (each_iterable_names, const_tag_names) = collect_each_and_const_names(&ctx.ast.html);
        immutable_names.retain(|n| !each_iterable_names.contains(*n) || const_tag_names.contains(*n));

        let all_immutable: HashSet<&str> = immutable_names.iter().copied()
            .chain(let_names.iter()
                .filter(|n| !mutable_lets.contains(*n) && !prop_names.contains(*n))
                .copied())
            .collect();

        let ast_immutable_stmts = check_immutability_ast(content, is_ts, &all_immutable);

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

        let all_declared: HashSet<&str> = all_immutable.iter().copied()
            .chain(mutable_lets.iter().copied())
            .chain(prop_names.iter().copied())
            .collect();

        for &(offset, ref full_text) in &reactive_stmts {
            let after = full_text[2..].trim_start();
            let rhs = if let Some(eq) = after.find('=') {
                let lhs = after[..eq].trim();
                let post = &after[eq + 1..];
                if post.starts_with('=') || post.starts_with('>') {
                    after
                } else if lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !lhs.is_empty() {
                    post
                } else {
                    after
                }
            } else {
                after
            };

            let mut ids = extract_identifiers(rhs);

            if rhs == after {
                let write_targets = extract_assignment_targets(rhs);
                if !write_targets.is_empty() {
                    let mut write_counts: HashMap<&str, usize> = HashMap::new();
                    for t in &write_targets {
                        *write_counts.entry(t).or_insert(0) += 1;
                    }
                    let mut id_counts: HashMap<String, usize> = HashMap::new();
                    for id in &ids {
                        *id_counts.entry(id.clone()).or_insert(0) += 1;
                    }
                    ids.retain(|id| {
                        id_counts.get(id).copied().unwrap_or(0) > write_counts.get(id.as_str()).copied().unwrap_or(0)
                    });
                }
            }

            if ids.iter().any(|id| id.starts_with('$')) { continue; }
            if ids.iter().any(|id| reactive_decl_names.contains(id.as_str())) { continue; }

            let referenced: Vec<&str> = ids.iter()
                .filter(|id| all_declared.contains(id.as_str()))
                .map(|s| s.as_str())
                .collect();

            let local_names = collect_local_names(rhs);
            let has_unknown = ids.iter().any(|id| {
                !all_declared.contains(id.as_str()) && !local_names.contains(id.as_str())
            });

            let text_flag = if has_unknown {
                false
            } else if !referenced.is_empty() {
                referenced.iter().all(|v| all_immutable.contains(v))
            } else if ids.is_empty() && rhs != after {
                true
            } else {
                false
            };

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
                let after = full_text[2..].trim_start();
                let body_off = full_text.len() - after.len();
                let base = content_offset + offset;
                let end = base + full_text.len();
                let diag_start = if let Some(eq) = after.find('=') {
                    let lhs = after[..eq].trim();
                    let post = &after[eq + 1..];
                    if !post.starts_with('=') && !post.starts_with('>')
                        && !lhs.is_empty() && lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                    {
                        let rhs_text = full_text[body_off + eq + 1..].trim_start();
                        base + full_text.len() - rhs_text.len()
                    } else {
                        base + body_off
                    }
                } else {
                    base + body_off
                };
                ctx.diagnostic(
                    "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
                    oxc::span::Span::new(diag_start as u32, end as u32),
                );
            }
        }
    }
}

fn collect_reactive_stmts(content: &str) -> Vec<(usize, &str)> {
    let mut stmts = Vec::new();
    let mut off = 0;
    for line in content.lines() {
        if line.trim().starts_with("$:") {
            let start_offset = off + (line.len() - line.trim_start().len());
            let stmt_end = find_statement_end(content, start_offset);
            stmts.push((start_offset, &content[start_offset..stmt_end]));
        }
        off += line.len() + 1;
    }
    stmts
}

fn find_statement_end(content: &str, start: usize) -> usize {
    let bytes = content.as_bytes();
    let mut i = start;
    if i + 2 <= bytes.len() && &content[i..i+2] == "$:" { i += 2; }
    i = content.len() - content[i..].trim_start().len();
    let mut db = 0i32;
    let mut dp = 0i32;
    let mut dk = 0i32;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                if i < bytes.len() { i += 1; }
                continue;
            }
            b'\'' | b'"' => { i = skip_simple_string(bytes, i); continue; }
            b'`' => { let (end, _) = skip_template_literal(bytes, i); i = end; continue; }
            b'{' => db += 1,
            b'}' => {
                db -= 1;
                if db < 0 { return i; }
                if db == 0 && dp == 0 && dk == 0 {
                    let mut j = i + 1;
                    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') { j += 1; }
                    if j < bytes.len() && matches!(bytes[j], b'[' | b'.' | b'(') { i += 1; continue; }
                    return j;
                }
            }
            b'(' => dp += 1,
            b')' => dp -= 1,
            b'[' => dk += 1,
            b']' => dk -= 1,
            b';' if db == 0 && dp == 0 && dk == 0 => return i + 1,
            b'\n' if db == 0 && dp == 0 && dk == 0 => {
                let before = content[start..i].trim_end();
                let after = content[i + 1..].trim_start();
                let ea = before.ends_with('=') && !before.ends_with("==");
                if before.ends_with('?') || before.ends_with(':') || before.ends_with("||")
                    || before.ends_with("&&") || before.ends_with('+') || before.ends_with('-')
                    || before.ends_with(',') || before.ends_with('\\') || before.ends_with("??")
                    || ea || after.starts_with('?') || after.starts_with(':')
                    || after.starts_with("||") || after.starts_with("&&") || after.starts_with("??")
                    || after.starts_with('.') || after.starts_with("?.") {
                    i += 1; continue;
                }
                return i;
            }
            _ => {}
        }
        i += 1;
    }
    content.len()
}

fn extract_destructured_names(pattern: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let close = if pattern.starts_with('{') { '}' } else { ']' };
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
            let local = if let Some(colon) = part.find(':') {
                part[colon + 1..].trim()
            } else {
                part
            };
            let local = local.split(':').next().unwrap_or(local).trim();
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

fn extract_multiline_imports<'a>(content: &'a str, immutable_names: &mut HashSet<&'a str>) {
    let mut search = 0;
    while search < content.len() {
        let rest = &content[search..];
        let Some(import_pos) = rest.find("import ").or_else(|| rest.find("import\t")) else { break };
        let abs = search + import_pos;
        let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
        if { let b = content[line_start..abs].trim(); !b.is_empty() && b != "export" } {
            search = abs + 7; continue;
        }
        let rest = &content[abs..];
        let first_nl = rest.find('\n').unwrap_or(rest.len());
        if rest[..first_nl].contains('{') && !rest[..first_nl].contains('}') {
            let mut end = abs + first_nl;
            let mut found = false;
            for _ in 0..20 {
                if end >= content.len() { break; }
                let line_end = content[end + 1..].find('\n').map(|p| end + 1 + p).unwrap_or(content.len());
                let line = content[end + 1..line_end].trim();
                if line.starts_with("} from ") || line.contains("} from '") || line.contains("} from \"") {
                    end = line_end; found = true; break;
                }
                if !line.starts_with("//") && !line.is_empty() && !line.ends_with(',')
                    && !line.starts_with('}') && line.contains('=') { break; }
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
        search = abs + 7;
    }
}

fn extract_import_names(line: &str) -> Vec<&str> {
    let mut names = Vec::new();
    if let Some(from_pos) = line.find(" from ") {
        let mut import_part = line[7..from_pos].trim();
        if import_part.starts_with("type ") && import_part[5..].trim_start().starts_with('{') {
            import_part = import_part[5..].trim_start();
        }
        if !import_part.starts_with('{') && !import_part.starts_with('*') {
            let end = import_part.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                .unwrap_or(import_part.len());
            let name = &import_part[..end];
            if !name.is_empty() { names.push(name); }
        }
        if let Some(open) = import_part.find('{') {
            if let Some(close) = import_part.find('}') {
                for part in import_part[open+1..close].split(',') {
                    let part = part.trim();
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

    let destructure_patterns = [
        format!("[{}]", var),    // [var] = ...
        format!(", {}]", var),   // [a, var] = ...
        format!("[{},", var),    // [var, b] = ...
        format!(": {} }}", var), // { x: var } = ...
        format!(": {}}}", var),  // {x:var} = ...
    ];
    for pat in &destructure_patterns {
        for (pos, _) in content.match_indices(pat.as_str()) {
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

fn has_member_write(content: &str, var: &str) -> bool {
    let dot_pat = format!("{}.", var);
    let bracket_pat = format!("{}[", var);

    let skip_brackets = |bytes: &[u8], i: &mut usize| {
        let mut d = 1; *i += 1;
        while *i < bytes.len() && d > 0 {
            match bytes[*i] { b'[' => d += 1, b']' => d -= 1, _ => {} }
            *i += 1;
        }
    };
    for pat in &[&dot_pat, &bracket_pat] {
        let is_bracket = pat == &&bracket_pat;
        for (pos, _) in content.match_indices(pat.as_str()) {
            if pos > 0 {
                let prev = content.as_bytes()[pos - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' { continue; }
            }
            let rest = &content[pos + pat.len()..];
            let mut i = 0;
            let bytes = rest.as_bytes();
            if is_bracket {
                skip_brackets(bytes, &mut i);
            } else {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            }
            while i < bytes.len() {
                match bytes[i] {
                    b'.' => { i += 1; while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; } }
                    b'[' => skip_brackets(bytes, &mut i),
                    _ => break,
                }
            }
            while i < bytes.len() && bytes[i].is_ascii_whitespace() { i += 1; }
            if i < bytes.len() {
                match bytes[i] {
                    b'=' if i + 1 < bytes.len() && bytes[i + 1] != b'=' => return true,
                    b'+' | b'-' if i + 1 < bytes.len() && bytes[i + 1] == bytes[i] => return true,
                    b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'
                        if i + 1 < bytes.len() && bytes[i + 1] == b'=' => return true,
                    _ => {}
                }
            }
        }
    }
    false
}

fn extract_assignment_targets(expr: &str) -> HashSet<&str> {
    let mut targets = HashSet::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => { i = skip_simple_string(bytes, i); continue; }
            b'`' => { let (end, _) = skip_template_literal(bytes, i); i = end; continue; }
            b'{' | b'(' | b'[' => { depth += 1; i += 1; }
            b'}' | b')' | b']' => { depth -= 1; i += 1; }
            b'=' if i + 1 < bytes.len() && bytes[i + 1] != b'=' && bytes[i + 1] != b'>' => {
                let mut j = i;
                while j > 0 && bytes[j - 1].is_ascii_whitespace() { j -= 1; }
                let end = j;
                while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_' || bytes[j - 1] == b'$') { j -= 1; }
                if j < end && (j == 0 || (bytes[j - 1] != b'.' && !bytes[j - 1].is_ascii_alphanumeric() && bytes[j - 1] != b'_')) {
                    targets.insert(&expr[j..end]);
                }
                i += 1;
            }
            _ => { i += 1; }
        }
    }
    targets
}

fn collect_local_names(expr: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => { i = skip_simple_string(bytes, i); continue; }
            b'`' => { let (end, _) = skip_template_literal(bytes, i); i = end; continue; }
            b'=' if i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                let mut j = i;
                while j > 0 && bytes[j - 1].is_ascii_whitespace() { j -= 1; }
                if j > 0 && bytes[j - 1] == b')' {
                    let mut depth = 1;
                    let mut k = j - 2;
                    while depth > 0 {
                        match bytes[k] { b')' => depth += 1, b'(' => depth -= 1, _ => {} }
                        if depth > 0 { if k == 0 { break; } k -= 1; }
                    }
                    extract_param_names(&expr[k + 1..j - 1], &mut names);
                } else {
                    let end = j;
                    while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_' || bytes[j - 1] == b'$') { j -= 1; }
                    if j < end { names.insert(expr[j..end].to_string()); }
                }
                i += 2; continue;
            }
            _ => {}
        }
        if i + 8 < bytes.len() && expr.is_char_boundary(i) && expr.is_char_boundary(i + 8) && &expr[i..i + 8] == "function" {
            let rest = expr[i + 8..].trim_start();
            let offset = expr.len() - rest.len();
            if let Some(open) = rest.find('(') {
                if let Some(close) = rest[open..].find(')') {
                    extract_param_names(&rest[open + 1..open + close], &mut names);
                    i = offset + open + close + 1; continue;
                }
            }
        }
        for kw in &["const ", "let ", "var "] {
            if i + kw.len() <= bytes.len() && expr.is_char_boundary(i) && expr.is_char_boundary(i + kw.len())
                && &expr[i..i + kw.len()] == *kw && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
                let rest = expr[i + kw.len()..].trim_start();
                let rest_offset = expr.len() - rest.len();
                let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').unwrap_or(rest.len());
                if end > 0 { names.insert(rest[..end].to_string()); }
                i = rest_offset + end; break;
            }
        }
        i += 1;
    }
    names
}

fn extract_param_names(params: &str, names: &mut HashSet<String>) {
    for p in params.split(',').map(str::trim) {
        if matches!(p.as_bytes().first(), Some(b'{' | b'[' | b'.')) { continue; }
        let name = p.split(|c: char| c == ':' || c == '=').next().unwrap_or("").trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            names.insert(name.to_string());
        }
    }
}

fn skip_template_literal(bytes: &[u8], mut i: usize) -> (usize, Vec<(usize, usize)>) {
    let mut interpolations = Vec::new();
    i += 1;
    while i < bytes.len() && bytes[i] != b'`' {
        if bytes[i] == b'\\' { i += 2; continue; }
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            i += 2;
            let start = i;
            let mut d = 1;
            while i < bytes.len() && d > 0 {
                match bytes[i] { b'{' => d += 1, b'}' => d -= 1, _ => {} }
                if d > 0 { i += 1; }
            }
            interpolations.push((start, i));
            if i < bytes.len() { i += 1; }
            continue;
        }
        i += 1;
    }
    if i < bytes.len() { i += 1; }
    (i, interpolations)
}

fn is_js_keyword_or_builtin(id: &str) -> bool {
    matches!(id, "true" | "false" | "null" | "undefined" | "new" | "typeof"
        | "if" | "else" | "return" | "const" | "let" | "var" | "function"
        | "class" | "this" | "console" | "Math" | "JSON" | "Object" | "Array"
        | "String" | "Number" | "Boolean" | "Date" | "Error" | "Promise"
        | "Map" | "Set" | "RegExp" | "Symbol" | "BigInt" | "Infinity" | "NaN"
        | "void" | "delete" | "instanceof" | "in" | "of" | "switch" | "case"
        | "break" | "continue" | "throw" | "try" | "catch" | "finally"
        | "for" | "while" | "do" | "async" | "await" | "yield"
        | "satisfies" | "as" | "super" | "with" | "debugger"
        | "default" | "export" | "from")
}

fn skip_simple_string(bytes: &[u8], i: usize) -> usize {
    let q = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() && bytes[j] != q {
        if bytes[j] == b'\\' { j += 1; }
        j += 1;
    }
    if j < bytes.len() { j + 1 } else { j }
}

fn extract_identifiers(expr: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
                if i + 1 < bytes.len() { i += 2; }
            }
            b'\'' | b'"' => { i = skip_simple_string(bytes, i); }
            b'`' => {
                let (end, interps) = skip_template_literal(bytes, i);
                for (s, e) in interps { ids.extend(extract_identifiers(&expr[s..e])); }
                i = end;
            }
            b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
                if i > 0 && bytes[i - 1] == b'.' && !(i >= 3 && bytes[i - 2] == b'.' && bytes[i - 3] == b'.') {
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                    continue;
                }
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') { i += 1; }
                let id = &expr[start..i];
                if !is_js_keyword_or_builtin(id) {
                    let next = expr[i..].trim_start();
                    let is_obj_key = next.starts_with(':') && !next.starts_with("::") && {
                        let b = expr[..start].trim_end();
                        b.ends_with('{') || b.ends_with(',') || b.ends_with('\n')
                    };
                    if !is_obj_key { ids.push(id.to_string()); }
                }
            }
            _ => { i += 1; }
        }
    }
    ids
}

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

            if name.starts_with('$') { has_store_ref = true; continue; }

            let parent_id = nodes.parent_id(desc.id());
            if let AstKind::StaticMemberExpression(member) = nodes.kind(parent_id) {
                if member.property.span == ident.span { continue; }
            }

            if is_simple_assign {
                if let AstKind::AssignmentExpression(assign) = nodes.kind(parent_id) {
                    if assign.left.span().start == ident.span.start { continue; }
                }
            }

            if let Some(ref_id) = ident.reference_id.get() {
                let reference = scoping.get_reference(ref_id);
                if reference.is_write() && !reference.is_read() {
                    continue;
                }
            }

            let symbol = ident.reference_id.get().and_then(|r| scoping.get_reference(r).symbol_id());
            match symbol {
                Some(sym) => {
                    has_any_ref = true;
                    if scoping.symbol_scope_id(sym) == root_scope
                        && !text_immutable.contains(scoping.symbol_name(sym))
                    {
                        all_refs_immutable = false;
                    }
                }
                None => {
                    if !is_known_js_global(name) { all_refs_immutable = false; }
                }
            }
        }

        if has_store_ref { continue; }
        if has_any_ref && all_refs_immutable {
            result.insert(stmt_start);
        }
    }

    result
}

fn collect_each_and_const_names(fragment: &crate::ast::Fragment) -> (HashSet<String>, HashSet<String>) {
    let (mut each_names, mut const_names) = (HashSet::new(), HashSet::new());
    let is_ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$');
    walk_template_nodes(fragment, &mut |node| match node {
        TemplateNode::EachBlock(each) => {
            let e = each.expression.trim();
            if is_ident(e) { each_names.insert(e.to_string()); }
        }
        TemplateNode::ConstTag(ct) => {
            if let Some(eq) = ct.declaration.find('=') {
                let lhs = ct.declaration[..eq].trim();
                if is_ident(lhs) { const_names.insert(lhs.to_string()); }
            }
        }
        _ => {}
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
