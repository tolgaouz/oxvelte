//! `svelte/consistent-selector-style` — enforce consistent style selector usage
//! (e.g. prefer class selectors over element selectors).

use crate::linter::{LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};
use oxc::span::Span;
use std::collections::{HashMap, HashSet};

pub struct ConsistentSelectorStyle;

/// Collect class, id, and element usage from the template in a single walk.
fn collect_template_info(html: &[TemplateNode]) -> (
    HashMap<String, (usize, HashSet<String>)>,
    HashMap<String, String>,
    HashMap<String, usize>,
) {
    let mut class_usage: HashMap<String, (usize, HashSet<String>)> = HashMap::new();
    let mut id_usage: HashMap<String, String> = HashMap::new();
    let mut element_usage: HashMap<String, usize> = HashMap::new();
    walk_template_nodes_slice(html, &mut |node| {
        if let TemplateNode::Element(el) = node {
            *element_usage.entry(el.name.clone()).or_insert(0) += 1;
            let is_component = el.name.starts_with(|c: char| c.is_uppercase()) || el.name.contains('.');
            for attr in &el.attributes {
                if let Attribute::NormalAttribute { name, value, .. } = attr {
                    if name == "class" && !is_component {
                        if let AttributeValue::Static(val) = value {
                            for cls in val.split_whitespace() {
                                let entry = class_usage.entry(cls.to_string()).or_insert((0, HashSet::new()));
                                entry.0 += 1;
                                entry.1.insert(el.name.clone());
                            }
                        }
                    } else if name == "id" {
                        if let AttributeValue::Static(val) = value {
                            id_usage.insert(val.clone(), el.name.clone());
                        }
                    }
                }
            }
        }
    });
    (class_usage, id_usage, element_usage)
}

/// Check if an element with a class is inside an each block or component (making class appropriate).
fn class_is_in_iteration_or_component(html: &[TemplateNode], class_name: &str) -> bool {
    fn check_nodes(nodes: &[TemplateNode], class_name: &str, in_iteration: bool) -> bool {
        for node in nodes {
            match node {
                TemplateNode::Element(el) => {
                    let has_class = el.attributes.iter().any(|a| {
                        if let Attribute::NormalAttribute { name, value, .. } = a {
                            if name == "class" {
                                if let AttributeValue::Static(val) = value {
                                    return val.split_whitespace().any(|c| c == class_name);
                                }
                            }
                        }
                        false
                    });
                    if has_class && in_iteration {
                        return true;
                    }
                    // Check if component (starts with uppercase)
                    let is_component = el.name.starts_with(|c: char| c.is_uppercase());
                    if check_nodes(&el.children, class_name, in_iteration || is_component) {
                        return true;
                    }
                }
                TemplateNode::EachBlock(each) => {
                    if check_nodes(&each.body.nodes, class_name, true) {
                        return true;
                    }
                    if let Some(alt) = &each.fallback {
                        if check_nodes(&alt.nodes, class_name, true) {
                            return true;
                        }
                    }
                }
                TemplateNode::IfBlock(ib) => {
                    if check_nodes(&ib.consequent.nodes, class_name, in_iteration) {
                        return true;
                    }
                    if let Some(alt) = &ib.alternate {
                        if check_nodes(&[*alt.clone()], class_name, in_iteration) {
                            return true;
                        }
                    }
                }
                TemplateNode::AwaitBlock(ab) => {
                    if let Some(p) = &ab.pending {
                        if check_nodes(&p.nodes, class_name, in_iteration) { return true; }
                    }
                    if let Some(t) = &ab.then {
                        if check_nodes(&t.nodes, class_name, in_iteration) { return true; }
                    }
                    if let Some(c) = &ab.catch {
                        if check_nodes(&c.nodes, class_name, in_iteration) { return true; }
                    }
                }
                TemplateNode::KeyBlock(kb) => {
                    if check_nodes(&kb.body.nodes, class_name, in_iteration) { return true; }
                }
                TemplateNode::SnippetBlock(sb) => {
                    // Snippets can be called 0+ times, treat as iteration
                    if check_nodes(&sb.body.nodes, class_name, true) { return true; }
                }
                _ => {}
            }
        }
        false
    }
    check_nodes(html, class_name, false)
}

/// Check if a class is defined via class: directive (dynamic)
fn class_has_directive(html: &[TemplateNode], class_name: &str) -> bool {
    let mut found = false;
    walk_template_nodes_slice(html, &mut |node| {
        if let TemplateNode::Element(el) = node {
            for attr in &el.attributes {
                if let Attribute::Directive { kind: crate::ast::DirectiveKind::Class, name, .. } = attr {
                    if name == class_name {
                        found = true;
                    }
                }
            }
        }
    });
    found
}

fn walk_template_nodes_slice(nodes: &[TemplateNode], f: &mut impl FnMut(&TemplateNode)) {
    for node in nodes {
        f(node);
        match node {
            TemplateNode::Element(el) => walk_template_nodes_slice(&el.children, f),
            TemplateNode::IfBlock(ib) => {
                walk_template_nodes_slice(&ib.consequent.nodes, f);
                if let Some(alt) = &ib.alternate {
                    f(alt);
                }
            }
            TemplateNode::EachBlock(each) => {
                walk_template_nodes_slice(&each.body.nodes, f);
                if let Some(alt) = &each.fallback {
                    walk_template_nodes_slice(&alt.nodes, f);
                }
            }
            TemplateNode::AwaitBlock(ab) => {
                if let Some(p) = &ab.pending { walk_template_nodes_slice(&p.nodes, f); }
                if let Some(t) = &ab.then { walk_template_nodes_slice(&t.nodes, f); }
                if let Some(c) = &ab.catch { walk_template_nodes_slice(&c.nodes, f); }
            }
            TemplateNode::KeyBlock(kb) => walk_template_nodes_slice(&kb.body.nodes, f),
            TemplateNode::SnippetBlock(sb) => walk_template_nodes_slice(&sb.body.nodes, f),
            _ => {}
        }
    }
}

impl Rule for ConsistentSelectorStyle {
    fn name(&self) -> &'static str {
        "svelte/consistent-selector-style"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let style = match &ctx.ast.css {
            Some(s) => s,
            None => return,
        };

        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        // Parse allowed styles (priority list)
        let allowed_styles: Vec<String> = opts
            .and_then(|o| o.get("style"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let check_global = opts
            .and_then(|o| o.get("checkGlobal"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // The first style in the list is the preferred one, default to "id"
        let default_preferred = "id".to_string();
        let preferred = if allowed_styles.is_empty() { &default_preferred } else { &allowed_styles[0] };

        // Compute priority positions (lower index = higher priority)
        // Vendor default priority: type(0) > id(1) > class(2)
        let id_pos = allowed_styles.iter().position(|s| s == "id").or(if allowed_styles.is_empty() { Some(1) } else { None });
        let class_pos = allowed_styles.iter().position(|s| s == "class").or(if allowed_styles.is_empty() { Some(2) } else { None });
        let type_pos = allowed_styles.iter().position(|s| s == "type").or(if allowed_styles.is_empty() { Some(0) } else { None });

        // Flag selector if there's a higher-priority (lower position) alternative available
        // Class selectors can potentially be replaced by id (if single use) or type
        let flag_class = !allowed_styles.is_empty() || check_global;
        // Type selectors flagged only when id has higher priority
        let flag_type = type_pos.map_or(false, |tp| id_pos.map_or(false, |ip| ip < tp));
        // Id selectors flagged only when type or class has higher priority (unusual but possible)
        let flag_id = id_pos.map_or(false, |ip| {
            type_pos.map_or(false, |tp| tp < ip) || class_pos.map_or(false, |cp| cp < ip)
        });

        let css = &style.content;
        let base = style.span.start as usize;

        // Collect template usage information for smart analysis
        let (class_usage, id_usage, element_usage) = collect_template_info(&ctx.ast.html.nodes);
        let html_nodes = &ctx.ast.html.nodes;

        const ELEMENT_SELECTORS: &[&str] = &[
            "div", "span", "p", "a", "ul", "ol", "li", "h1", "h2", "h3",
            "h4", "h5", "h6", "table", "tr", "td", "th", "section", "article",
            "header", "footer", "nav", "main", "aside", "form", "input",
            "button", "select", "textarea", "img", "label", "b", "i", "em",
            "strong", "small", "pre", "code", "blockquote", "figure",
            "figcaption", "details", "summary", "mark", "time", "abbr",
            "cite", "q", "s", "u", "sub", "sup", "dl", "dt", "dd",
        ];

        // Helper closure: returns Some(suggested_style) if a class selector should be flagged.
        // Returns None if the selector is fine.
        let check_class_selector = |class_name: &str| -> Option<String> {
            if !flag_class { return None; }
            let (count, ref element_types) = class_usage.get(class_name)
                .map(|(c, t)| (*c, t.clone()))
                .unwrap_or((0, HashSet::new()));
            // If class not found in template, it might be dynamic - skip
            if count == 0 { return None; }
            let can_be_id = count <= 1
                && !class_is_in_iteration_or_component(html_nodes, class_name)
                && !class_has_directive(html_nodes, class_name);
            // Can use type selector only if all elements with this class are the same type
            // AND the type selector would match exactly the same elements (no extra elements of that type)
            let can_be_type = if element_types.len() == 1 {
                let the_type = element_types.iter().next().unwrap();
                let total_of_type = element_usage.get(the_type).copied().unwrap_or(0);
                total_of_type == count
            } else {
                false
            };

            // Find the best higher-priority alternative
            // Check id first (always higher than class if in the list)
            if can_be_id {
                if let (Some(ip), Some(cp)) = (id_pos, class_pos) {
                    if ip < cp {
                        return Some("ID".to_string());
                    }
                }
            }

            // Check type (if type has higher priority than class in config)
            if can_be_type {
                if let (Some(tp), Some(cp)) = (type_pos, class_pos) {
                    if tp < cp {
                        return Some("element type".to_string());
                    }
                }
            }

            // No higher-priority alternative available
            None
        };

        // Helper closure for type (element) selectors
        let check_type_selector = |elem_name: &str| -> Option<String> {
            // Check if id has higher priority than type and element can be targeted by id
            let elem_count = element_usage.get(elem_name).copied().unwrap_or(0);
            let can_be_id = elem_count <= 1;
            if can_be_id {
                if let (Some(ip), Some(tp)) = (id_pos, type_pos) {
                    if ip < tp {
                        return Some("ID".to_string());
                    }
                }
            }
            None
        };

        // Helper closure for id selectors
        let check_id_selector = |id_name: &str| -> Option<String> {
            if !flag_id { return None; }
            // Find the element type for this id
            if let Some(elem_type) = id_usage.get(id_name) {
                // Check if type selector would be unique (only one element of this type)
                let elem_count = element_usage.get(elem_type).copied().unwrap_or(0);
                if elem_count <= 1 {
                    if let (Some(tp), Some(ip)) = (type_pos, id_pos) {
                        if tp < ip {
                            return Some("element type".to_string());
                        }
                    }
                }
            }
            None
        };

        // When checkGlobal is on, scan for :global() and :global { } selectors
        if check_global {
            let mut in_global_block = false;
            let mut global_brace_depth = 0i32;
            let mut byte_offset = 0usize;

            for line in css.split('\n') {
                let trimmed = line.trim();
                let leading = line.len() - line.trim_start().len();

                // Track :global { } blocks (bare :global without parens)
                if !in_global_block && trimmed.starts_with(":global") && !trimmed.contains('(') && trimmed.contains('{') {
                    in_global_block = true;
                    global_brace_depth = 1;
                    byte_offset += line.len() + 1;
                    continue;
                }
                if in_global_block {
                    for ch in trimmed.chars() {
                        if ch == '{' { global_brace_depth += 1; }
                        if ch == '}' { global_brace_depth -= 1; }
                    }
                    if global_brace_depth <= 0 {
                        in_global_block = false;
                        byte_offset += line.len() + 1;
                        continue;
                    }

                    // Check selectors inside :global { } blocks
                    if !trimmed.is_empty() && !trimmed.starts_with('}')
                        && !trimmed.ends_with(';') && !trimmed.starts_with("/*")
                        && !trimmed.starts_with("//")
                    {
                        // Extract selector part (before {)
                        let sel_text = if let Some(brace) = trimmed.find('{') {
                            &trimmed[..brace]
                        } else {
                            trimmed
                        };
                        flag_selectors_in_text(ctx, sel_text, base + byte_offset + leading,
                            &check_class_selector, &check_type_selector, &check_id_selector, ELEMENT_SELECTORS);
                    }

                    byte_offset += line.len() + 1;
                    continue;
                }

                // Check for :global(.selector) patterns
                let mut search_pos = 0usize;
                while let Some(gpos) = line[search_pos..].find(":global(") {
                    let abs_pos = search_pos + gpos;
                    let inner_start = abs_pos + 8;
                    if let Some(close_paren) = find_matching_paren(&line[inner_start..]) {
                        let inner = &line[inner_start..inner_start + close_paren];
                        flag_selectors_in_text(ctx, inner, base + byte_offset + inner_start,
                            &check_class_selector, &check_type_selector, &check_id_selector, ELEMENT_SELECTORS);
                    }
                    search_pos = inner_start;
                }

                byte_offset += line.len() + 1;
            }
        }

        // Scan non-global CSS selectors only when style config is explicitly provided
        if allowed_styles.is_empty() { return; }

        let mut byte_offset = 0usize;
        let mut brace_depth = 0i32;

        for line in css.split('\n') {
            let trimmed = line.trim();
            let leading = line.len() - line.trim_start().len();

            // A selector line is at brace depth 0 (not inside a declaration block)
            let is_selector_line = brace_depth == 0
                && !trimmed.is_empty()
                && !trimmed.starts_with("/*") && !trimmed.starts_with("//")
                && !trimmed.starts_with('}')
                && !trimmed.ends_with(';');

            if is_selector_line {
                let selector_text = if let Some(brace_pos) = trimmed.find('{') {
                    &trimmed[..brace_pos]
                } else {
                    trimmed
                };

                // Split on commas to handle multiple selectors
                for sel_part in selector_text.split(',') {
                    let sel = sel_part.trim();
                    if sel.is_empty() { continue; }

                    // Find this selector's position in the original line
                    let sel_offset_in_line = if let Some(pos) = line.find(sel) {
                        pos
                    } else {
                        leading
                    };

                    flag_selectors_in_text(ctx, sel, base + byte_offset + sel_offset_in_line,
                        &check_class_selector, &check_type_selector, &check_id_selector, ELEMENT_SELECTORS);
                }
            }

            // Update brace depth after checking selectors
            for ch in trimmed.chars() {
                if ch == '{' { brace_depth += 1; }
                if ch == '}' { brace_depth -= 1; if brace_depth < 0 { brace_depth = 0; } }
            }

            byte_offset += line.len() + 1;
        }
    }
}

fn find_matching_paren(s: &str) -> Option<usize> {
    let mut depth = 1i32;
    for (i, ch) in s.char_indices() {
        if ch == '(' { depth += 1; }
        if ch == ')' { depth -= 1; if depth == 0 { return Some(i); } }
    }
    None
}

/// Flag class, id, and element type selectors in text.
fn flag_selectors_in_text(
    ctx: &mut LintContext,
    text: &str,
    base_offset: usize,
    check_class: &dyn Fn(&str) -> Option<String>,
    check_type: &dyn Fn(&str) -> Option<String>,
    check_id: &dyn Fn(&str) -> Option<String>,
    element_selectors: &[&str],
) {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            b'.' => {
                // Class selector
                let dot_pos = pos;
                pos += 1;
                while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'-' || bytes[pos] == b'_') {
                    pos += 1;
                }
                if pos > dot_pos + 1 {
                    let class_name = &text[dot_pos + 1..pos];
                    if let Some(suggested) = check_class(class_name) {
                        ctx.diagnostic(
                            format!("Selector should select by {} instead of class", suggested),
                            Span::new((base_offset + dot_pos) as u32, (base_offset + pos) as u32),
                        );
                    }
                }
            }
            b'#' => {
                // ID selector
                let hash_pos = pos;
                pos += 1;
                while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'-' || bytes[pos] == b'_') {
                    pos += 1;
                }
                if pos > hash_pos + 1 {
                    let id_name = &text[hash_pos + 1..pos];
                    if let Some(suggested) = check_id(id_name) {
                        ctx.diagnostic(
                            format!("Selector should select by {} instead of ID", suggested),
                            Span::new((base_offset + hash_pos) as u32, (base_offset + pos) as u32),
                        );
                    }
                }
            }
            b'[' => {
                // Attribute selector - skip
                while pos < bytes.len() && bytes[pos] != b']' { pos += 1; }
                if pos < bytes.len() { pos += 1; }
            }
            b':' => {
                // Pseudo-class/element - skip
                pos += 1;
                if pos < bytes.len() && bytes[pos] == b':' { pos += 1; }
                while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'-' || bytes[pos] == b'_') {
                    pos += 1;
                }
                if pos < bytes.len() && bytes[pos] == b'(' {
                    let mut depth = 1;
                    pos += 1;
                    while pos < bytes.len() && depth > 0 {
                        if bytes[pos] == b'(' { depth += 1; }
                        if bytes[pos] == b')' { depth -= 1; }
                        pos += 1;
                    }
                }
            }
            b' ' | b'>' | b'+' | b'~' | b'*' => {
                pos += 1;
            }
            _ if bytes[pos].is_ascii_alphabetic() || bytes[pos] == b'_' => {
                // Possible element name
                let start = pos;
                while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'-' || bytes[pos] == b'_') {
                    pos += 1;
                }
                let name = &text[start..pos];
                if element_selectors.contains(&name) {
                    if let Some(suggested) = check_type(name) {
                        ctx.diagnostic(
                            format!("Selector should select by {} instead of element type", suggested),
                            Span::new((base_offset + start) as u32, (base_offset + pos) as u32),
                        );
                    }
                }
            }
            _ => {
                pos += 1;
            }
        }
    }
}
