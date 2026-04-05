//! `svelte/no-reactive-reassign` — disallow reassignment of reactive values.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use std::collections::HashSet;

pub struct NoReactiveReassign;

const MUTATING_METHODS: &[&str] = &[
    "push(", "pop(", "shift(", "unshift(", "splice(",
    "sort(", "reverse(", "fill(", "copyWithin(",
];

impl Rule for NoReactiveReassign {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-reassign"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let check_props = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("props"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            if !content.contains("$:") { return; }
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

            let mut reactive_vars = HashSet::new();
            let mut declared_vars = HashSet::new();

            let mut brace_depth: i32 = 0;

            for line in content.lines() {
                let trimmed = line.trim();

                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }

                let at_top_level = brace_depth == 0
                    || (brace_depth == 1 && trimmed.contains('{') && !trimmed.contains('}'));

                if at_top_level {
                    let decl_trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
                    for kw in &["let ", "var ", "const "] {
                        if decl_trimmed.starts_with(kw) {
                            let rest = &decl_trimmed[kw.len()..];
                            let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                                .unwrap_or(rest.len());
                            if name_end > 0 {
                                declared_vars.insert(rest[..name_end].to_string());
                            }
                        }
                    }
                }

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

            reactive_vars.retain(|v| !declared_vars.contains(v));

            if reactive_vars.is_empty() { return; }

            fn find_matching_paren_rev(content: &str, close_pos: usize) -> Option<usize> {
                let mut depth = 0i32;
                for (i, ch) in content[..=close_pos].char_indices().rev() {
                    match ch {
                        ')' => depth += 1,
                        '(' => { depth -= 1; if depth == 0 { return Some(i); } }
                        _ => {}
                    }
                }
                None
            }

            fn is_shadowed_in_scope(content: &str, pos: usize, var_name: &str) -> bool {
                let before = &content[..pos];
                let mut search_end = before.len();

                loop {
                    let mut depth: i32 = 0;
                    let mut func_start = None;
                    for (i, ch) in before[..search_end].char_indices().rev() {
                        match ch {
                            '}' => depth += 1,
                            '{' => { if depth == 0 { func_start = Some(i); break; } depth -= 1; }
                            _ => {}
                        }
                    }
                    let Some(brace_pos) = func_start else { return false };

                    let before_brace = content[..brace_pos].trim_end();
                    let is_control_flow = if before_brace.ends_with(')') {
                        find_matching_paren_rev(before_brace, before_brace.len() - 1).is_some_and(|ps| {
                            let bp = before_brace[..ps].trim_end();
                            ["if", "for", "while", "switch", "catch"].iter().any(|kw| bp.ends_with(kw))
                        })
                    } else {
                        ["else", "try", "finally"].iter().any(|kw| before_brace.ends_with(kw))
                    };
                    let is_function_scope = !is_control_flow && (
                        before_brace.ends_with(')')
                        || before_brace.ends_with("=>")
                        || (before_brace.contains('(') && before_brace.rfind(')').is_some_and(|p| {
                            let after_paren = before_brace[p + 1..].trim();
                            after_paren.is_empty() || after_paren.starts_with(':')
                        }))
                    );

                    if is_function_scope {
                        if let Some(paren_end) = before_brace.rfind(')') {
                            if let Some(paren_start) = find_matching_paren_rev(content, paren_end) {
                                let params = &content[paren_start + 1..paren_end];
                                for param in params.split(',') {
                                    let name = param.trim().split(|c: char| c == ':' || c == '=' || c == '?' || c == ' ')
                                        .next().unwrap_or("").trim();
                                    if name == var_name { return true; }
                                }
                            }
                        }
                        let scope_content = &content[brace_pos..pos];
                        for line in scope_content.lines() {
                            let t = line.trim();
                            for kw in &["const ", "let ", "var "] {
                                if let Some(rest) = t.strip_prefix(kw) {
                                    let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                                        .unwrap_or(rest.len());
                                    if end > 0 && &rest[..end] == var_name { return true; }
                                }
                            }
                        }
                    }
                    search_end = brace_pos;
                }
            }

            let suffixes: &[&str] = &[" =", "=", "++", "--", " +=", " -=", " *=", " /=", " %=", " &&=", " ||=", " ??="];
            for var in &reactive_vars {
                // Single scan for the var, then check suffixes at each match
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(var.as_str()) {
                    let abs = search_from + pos;
                    search_from = abs + var.len();

                    // Word boundary check
                    if abs > 0 && matches!(content.as_bytes()[abs - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'.') { continue; }
                    let after = &content[abs + var.len()..];

                    // Check which suffix matches
                    let matched_suffix = suffixes.iter().find(|s| after.starts_with(*s));
                    let Some(suffix) = matched_suffix else { continue };
                    let pattern_len = var.len() + suffix.len();

                    let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let line = content[line_start..].trim_start();
                    if line.starts_with("$:") || line.starts_with("//") || line.starts_with("/*")
                        || line.starts_with("const ") || line.starts_with("let ") || line.starts_with("var ")
                    { continue; }

                    if let Some(bo) = content[..abs].rfind('[') {
                        let inside = content[..abs].rfind(']').map_or(true, |bc| bo > bc);
                        if inside && after.trim_start().starts_with(']') { continue; }
                    }

                    if suffix.ends_with('=') {
                        let after_eq = abs + pattern_len;
                        if after_eq < content.len() && content.as_bytes()[after_eq] == b'=' { continue; }
                    }

                    if is_shadowed_in_scope(content, abs, var) { continue; }

                    let sp = content_offset + abs;
                    ctx.diagnostic(format!("Assignment to reactive value '{}'.", var),
                        oxc::span::Span::new(sp as u32, (sp + pattern_len) as u32));
                }
            }

            if check_props {
            for var in &reactive_vars {
                if !content.contains(var.as_str()) { continue; }
                // Single "var." scan for mutating methods, chained mutations, and ++/--
                {
                    let prefix = format!("{}.", var);
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(prefix.as_str()) {
                        let abs = search_from + pos;
                        search_from = abs + prefix.len();
                        if abs > 0 && matches!(content.as_bytes()[abs - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_') { continue; }
                        let after_prefix = &content[abs + prefix.len()..];

                        // Check direct mutating method: var.push(
                        if let Some(method) = MUTATING_METHODS.iter().find(|m| after_prefix.starts_with(*m)) {
                            let ls = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                            if !content[ls..].trim_start().starts_with("$:") && !is_shadowed_in_scope(content, abs, var) {
                                let sp = content_offset + abs;
                                ctx.diagnostic(format!("Assignment to reactive value '{}'.", var),
                                    oxc::span::Span::new(sp as u32, (sp + prefix.len() + method.len()) as u32));
                            }
                            continue;
                        }

                        // Follow property chain for chained mutations, assignments, and ++/--
                        let mut rest = after_prefix;
                        let mut chain_len = prefix.len();
                        loop {
                            let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
                            if end == 0 { break; }
                            rest = &rest[end..];
                            chain_len += end;

                            // Check for ++ and -- on property
                            if rest.starts_with("++") || rest.starts_with("--") {
                                let ls = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                                if !content[ls..].trim_start().starts_with("$:") && !is_shadowed_in_scope(content, abs, var) {
                                    let sp = content_offset + abs;
                                    ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                        oxc::span::Span::new(sp as u32, (sp + chain_len + 2) as u32));
                                }
                                break;
                            }

                            if rest.starts_with('.') || rest.starts_with("?.") {
                                let skip = if rest.starts_with("?.") { 2 } else { 1 };
                                rest = &rest[skip..];
                                chain_len += skip;
                                // Check for chained mutating method
                                for m in MUTATING_METHODS {
                                    if rest.starts_with(*m) {
                                        let ls = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                                        if !content[ls..].trim_start().starts_with("$:") && !is_shadowed_in_scope(content, abs, var) {
                                            let sp = content_offset + abs;
                                            ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                                oxc::span::Span::new(sp as u32, (sp + chain_len + m.len() - 1) as u32));
                                        }
                                    }
                                }
                            } else if let Some(close) = rest.strip_prefix('[').and_then(|r| r.find(']')) {
                                rest = &rest[close + 2..];
                                chain_len += close + 2;
                            } else {
                                break;
                            }
                        }
                    }
                }
                for pattern_base in &[format!("{}.", var), format!("{}?.", var), format!("{}[", var)] {
                    for (pos, _) in content.match_indices(pattern_base.as_str()) {
                        if pos > 0 && matches!(content.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_') { continue; }
                        let ls = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        if content[ls..].trim_start().starts_with("$:") || is_shadowed_in_scope(content, pos, var) { continue; }
                        let after = &content[pos + pattern_base.len()..];
                        let mut rest = if pattern_base.ends_with('[') {
                            after.find(']').map(|p| &after[p+1..]).unwrap_or("")
                        } else {
                            let end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
                            &after[end..]
                        };
                        loop {
                            if rest.starts_with('.') || rest.starts_with("?.") {
                                let skip = if rest.starts_with("?.") { 2 } else { 1 };
                                let r = &rest[skip..];
                                let end = r.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(r.len());
                                rest = &r[end..];
                            } else if rest.starts_with('[') {
                                rest = rest[1..].find(']').map(|p| &rest[p+2..]).unwrap_or("");
                            } else {
                                break;
                            }
                        }
                        let rest = rest.trim_start();
                        if rest.starts_with('=') && !rest.starts_with("==") {
                            let sp = content_offset + pos;
                            ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                oxc::span::Span::new(sp as u32, (sp + pattern_base.len()) as u32));
                        }
                    }
                }
                let delete_pattern = format!("delete {}", var);
                for (pos, _) in content.match_indices(&delete_pattern) {
                    let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let line = content[line_start..].trim_start();
                    if line.starts_with("$:") { continue; }
                    let sp = content_offset + pos;
                    ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                        oxc::span::Span::new(sp as u32, (sp + delete_pattern.len()) as u32));
                }
            }
            } // end if check_props (step 2b)

            for var in &reactive_vars {
                let destructure_patterns = [
                    format!("{} }} =", var),     // { foo: reactiveVar } =
                    format!("{}}} =", var),      // {reactiveVar} = (no space)
                    format!("{}] =", var),       // [reactiveVar] =
                    format!("{}]] =", var),      // [[reactiveVar]] = (nested)
                    format!("...{} }} =", var),  // { ...reactiveVar } =
                    format!("...{}] =", var),    // [...reactiveVar] =
                ];
                for pattern in &destructure_patterns {
                    if let Some(pos) = content.find(pattern.as_str()) {
                        let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = content[line_start..].trim_start();
                        if line.starts_with("$:") || line.starts_with("const ")
                            || line.starts_with("let ") || line.starts_with("var ") { continue; }

                        if pattern.ends_with("] =") && !pattern.ends_with("]] =") && !pattern.starts_with("...") {
                            let before = &content[..pos];
                            if let Some(bracket_pos) = before.rfind('[') {
                                let between = content[bracket_pos + 1..pos].trim();
                                if between.is_empty() {
                                    let before_bracket = content[..bracket_pos].trim_end();
                                    if !(before_bracket.ends_with('=')
                                        || before_bracket.ends_with(',')
                                        || before_bracket.ends_with(';')
                                        || before_bracket.ends_with('{')
                                        || before_bracket.ends_with('(')
                                        || before_bracket.is_empty()
                                        || before_bracket.ends_with('\n'))
                                    {
                                        continue;
                                    }
                                } else {
                                    if !between.contains(',') {
                                        continue; // computed property access
                                    }
                                }
                            }
                        }

                        let sp = content_offset + pos;
                        ctx.diagnostic(format!("Assignment to reactive value '{}'.", var),
                            oxc::span::Span::new(sp as u32, (sp + var.len()) as u32));
                        break; // Only report once per var per pattern type
                    }
                }
            }

            for var in &reactive_vars {
                let for_patterns = [
                    format!("for ({} ", var),
                    format!("for ({}", var),
                    format!("for (const {} ", var),
                    format!("for (let {} ", var),
                ];
                let member_for = if check_props {
                    vec![
                        format!("for ({}.", var),
                        format!("for (const {}.", var),
                        format!("for (let {}.", var),
                    ]
                } else {
                    vec![]
                };
                for pattern in member_for.iter().chain(for_patterns.iter()) {
                    if let Some(pos) = content.find(pattern.as_str()) {
                        let after = &content[pos + pattern.len()..];
                        if after.contains(" of ") || after.contains(" in ") {
                            let sp = content_offset + pos;
                            ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                oxc::span::Span::new(sp as u32, (sp + pattern.len()) as u32));
                        }
                    }
                }
            }

            if check_props { for var in &reactive_vars {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("$:") { continue; }
                    if trimmed.contains(&format!("? {} :", var))
                        || trimmed.contains(&format!("? {}", var))
                    {
                        if let Some(dot_pos) = trimmed.rfind(").") {
                            let after_dot = &trimmed[dot_pos + 2..];
                            let end = after_dot.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after_dot.len());
                            let rest = after_dot[end..].trim_start();
                            if rest.starts_with('=') && !rest.starts_with("==") {
                                if let Some(pos) = content.find(trimmed) {
                                    let sp = content_offset + pos;
                                    ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", var),
                                        oxc::span::Span::new(sp as u32, (sp + trimmed.len()) as u32));
                                }
                            }
                        }
                    }
                }
            }
            } // end if check_props (conditional member assignment)

            walk_template_nodes(&ctx.ast.html, &mut |node| {
                if let TemplateNode::Element(el) = node {
                    for attr in &el.attributes {
                        let expr_span = match attr {
                            Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } => Some(*span),
                            Attribute::NormalAttribute { span, value, .. } => {
                                match value {
                                    crate::ast::AttributeValue::Expression(_) => Some(*span),
                                    crate::ast::AttributeValue::Concat(_) => Some(*span),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some(span) = expr_span {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            let tmpl_suffixes: &[&str] = &[" = ", " += ", " -= ", " *= ", " /= ", " %= ", "++", "--"];
                            for var in &reactive_vars {
                                let pats: Vec<String> = tmpl_suffixes.iter().map(|s| format!("{}{}", var, s)).collect();
                                'next_var: for pat in &pats {
                                    for (pos, _) in region.match_indices(pat.as_str()) {
                                        if pos > 0 && matches!(region.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'.') { continue; }
                                        if pat.ends_with("= ") && pos + pat.len() - 1 < region.len() && region.as_bytes()[pos + pat.len() - 1] == b'=' { continue; }
                                        let before = &region[..pos];
                                        if before.matches('\'').count() % 2 != 0 || before.matches('"').count() % 2 != 0 { continue; }
                                        let ap = span.start as usize + pos;
                                        ctx.diagnostic(format!("Assignment to reactive value '{}'.", var),
                                            oxc::span::Span::new(ap as u32, (ap + var.len()) as u32));
                                        break 'next_var;
                                    }
                                }
                            }
                            if check_props { for var in &reactive_vars {
                                for prefix in &[var.clone(), format!("${}", var)] {
                                    for pat_start in &[format!("{}.", prefix), format!("{}[", prefix)] {
                                        for (pos, _) in region.match_indices(pat_start.as_str()) {
                                            if pos > 0 && matches!(region.as_bytes()[pos - 1], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') { continue; }
                                            let before = &region[..pos];
                                            if before.matches('\'').count() % 2 != 0 || before.matches('"').count() % 2 != 0 { continue; }
                                            let after = &region[pos + pat_start.len()..];
                                            let mut rest = if pat_start.ends_with('[') {
                                                after.find(']').map(|p| &after[p+1..]).unwrap_or("")
                                            } else {
                                                let end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
                                                &after[end..]
                                            };
                                            loop {
                                                if rest.starts_with('.') || rest.starts_with("?.") {
                                                    let skip = if rest.starts_with("?.") { 2 } else { 1 };
                                                    let r = &rest[skip..];
                                                    for m in MUTATING_METHODS {
                                                        if r.starts_with(*m) {
                                                            let ap = span.start as usize + pos;
                                                            ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", prefix),
                                                                oxc::span::Span::new(ap as u32, (ap + pat_start.len()) as u32));
                                                        }
                                                    }
                                                    let end = r.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(r.len());
                                                    rest = &r[end..];
                                                } else if rest.starts_with('[') {
                                                    rest = rest[1..].find(']').map(|p| &rest[p+2..]).unwrap_or("");
                                                } else { break; }
                                            }
                                            let rest = rest.trim_start();
                                            if rest.starts_with('=') && !rest.starts_with("==") {
                                                let ap = span.start as usize + pos;
                                                ctx.diagnostic(format!("Assignment to property of reactive value '{}'.", prefix),
                                                    oxc::span::Span::new(ap as u32, (ap + pat_start.len()) as u32));
                                                break;
                                            }
                                        }
                                    }
                                }
                            }}
                        }
                        if let Attribute::Directive { kind: DirectiveKind::Binding, name, span, .. } = attr {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            if let (Some(open), Some(close)) = (region.find('{'), region.find('}')) {
                                let bound = region[open+1..close].trim();
                                let base = bound.split('.').next().unwrap_or(bound);
                                if reactive_vars.contains(bound) || (reactive_vars.contains(base) && (check_props || !bound.contains('.'))) {
                                    ctx.diagnostic(format!("Assignment to reactive value '{}'.", base), *span);
                                }
                            } else if reactive_vars.contains(name.as_str()) {
                                ctx.diagnostic(format!("Assignment to reactive value '{}'.", name), *span);
                            }
                        }
                    }
                }
            });
        }
    }
}
