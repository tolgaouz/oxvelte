//! `svelte/consistent-selector-style` — enforce consistent style selector usage
//! (e.g. prefer class selectors over element selectors).

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};
use oxc::span::Span;
use std::collections::{HashMap, HashSet};

pub struct ConsistentSelectorStyle;

fn collect_template_info(html: &crate::ast::Fragment) -> (
    HashMap<String, (usize, HashSet<String>)>,
    HashMap<String, String>,
    HashMap<String, usize>,
) {
    let mut class_usage: HashMap<String, (usize, HashSet<String>)> = HashMap::new();
    let mut id_usage: HashMap<String, String> = HashMap::new();
    let mut element_usage: HashMap<String, usize> = HashMap::new();
    walk_template_nodes(html, &mut |node| {
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

fn class_is_in_iteration_or_component(html: &[TemplateNode], class_name: &str) -> bool {
    fn check(nodes: &[TemplateNode], cn: &str, iter: bool) -> bool {
        nodes.iter().any(|node| match node {
            TemplateNode::Element(el) => {
                let has = el.attributes.iter().any(|a| matches!(a,
                    Attribute::NormalAttribute { name, value: AttributeValue::Static(val), .. }
                    if name == "class" && val.split_whitespace().any(|c| c == cn)));
                (has && iter) || check(&el.children, cn, iter || el.name.starts_with(|c: char| c.is_uppercase()))
            }
            TemplateNode::EachBlock(each) => check(&each.body.nodes, cn, true)
                || each.fallback.as_ref().is_some_and(|f| check(&f.nodes, cn, true)),
            TemplateNode::IfBlock(ib) => check(&ib.consequent.nodes, cn, iter)
                || ib.alternate.as_ref().is_some_and(|alt| check(&[*alt.clone()], cn, iter)),
            TemplateNode::AwaitBlock(ab) => [&ab.pending, &ab.then, &ab.catch].iter()
                .any(|f| f.as_ref().is_some_and(|f| check(&f.nodes, cn, iter))),
            TemplateNode::KeyBlock(kb) => check(&kb.body.nodes, cn, iter),
            TemplateNode::SnippetBlock(sb) => check(&sb.body.nodes, cn, true),
            _ => false,
        })
    }
    check(html, class_name, false)
}

fn class_has_directive(html: &crate::ast::Fragment, class_name: &str) -> bool {
    let mut found = false;
    walk_template_nodes(html, &mut |node| {
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

        let allowed_styles: Vec<String> = opts
            .and_then(|o| o.get("style"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let check_global = opts
            .and_then(|o| o.get("checkGlobal"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let default_preferred = "id".to_string();
        let preferred = if allowed_styles.is_empty() { &default_preferred } else { &allowed_styles[0] };

        let id_pos = allowed_styles.iter().position(|s| s == "id").or(if allowed_styles.is_empty() { Some(1) } else { None });
        let class_pos = allowed_styles.iter().position(|s| s == "class").or(if allowed_styles.is_empty() { Some(2) } else { None });
        let type_pos = allowed_styles.iter().position(|s| s == "type").or(if allowed_styles.is_empty() { Some(0) } else { None });

        let flag_class = !allowed_styles.is_empty() || check_global;
        let flag_type = type_pos.map_or(false, |tp| id_pos.map_or(false, |ip| ip < tp));
        let flag_id = id_pos.map_or(false, |ip| {
            type_pos.map_or(false, |tp| tp < ip) || class_pos.map_or(false, |cp| cp < ip)
        });

        let css = &style.content;
        let base = style.span.start as usize;

        let (class_usage, id_usage, element_usage) = collect_template_info(&ctx.ast.html);
        let html = &ctx.ast.html;

        const ELEMENT_SELECTORS: &[&str] = &[
            "div", "span", "p", "a", "ul", "ol", "li", "h1", "h2", "h3",
            "h4", "h5", "h6", "table", "tr", "td", "th", "section", "article",
            "header", "footer", "nav", "main", "aside", "form", "input",
            "button", "select", "textarea", "img", "label", "b", "i", "em",
            "strong", "small", "pre", "code", "blockquote", "figure",
            "figcaption", "details", "summary", "mark", "time", "abbr",
            "cite", "q", "s", "u", "sub", "sup", "dl", "dt", "dd",
        ];

        let check_class_selector = |class_name: &str| -> Option<String> {
            if !flag_class { return None; }
            let (count, ref element_types) = class_usage.get(class_name)
                .map(|(c, t)| (*c, t.clone()))
                .unwrap_or((0, HashSet::new()));
            if count == 0 { return None; }
            let can_be_id = count <= 1
                && !class_is_in_iteration_or_component(&html.nodes, class_name)
                && !class_has_directive(html, class_name);
            let can_be_type = if element_types.len() == 1 {
                let the_type = element_types.iter().next().unwrap();
                let total_of_type = element_usage.get(the_type).copied().unwrap_or(0);
                total_of_type == count
            } else {
                false
            };

            if can_be_id {
                if let (Some(ip), Some(cp)) = (id_pos, class_pos) {
                    if ip < cp {
                        return Some("ID".to_string());
                    }
                }
            }

            if can_be_type {
                if let (Some(tp), Some(cp)) = (type_pos, class_pos) {
                    if tp < cp {
                        return Some("element type".to_string());
                    }
                }
            }

            None
        };

        let check_type_selector = |elem_name: &str| -> Option<String> {
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

        let check_id_selector = |id_name: &str| -> Option<String> {
            if !flag_id { return None; }
            if let Some(elem_type) = id_usage.get(id_name) {
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

        if check_global {
            let mut in_global_block = false;
            let mut global_brace_depth = 0i32;
            let mut byte_offset = 0usize;

            for line in css.split('\n') {
                let trimmed = line.trim();
                let leading = line.len() - line.trim_start().len();

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

                    if !trimmed.is_empty() && !trimmed.starts_with('}')
                        && !trimmed.ends_with(';') && !trimmed.starts_with("/*")
                        && !trimmed.starts_with("//")
                    {
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

        if allowed_styles.is_empty() { return; }

        let mut byte_offset = 0usize;
        let mut brace_depth = 0i32;

        for line in css.split('\n') {
            let trimmed = line.trim();
            let leading = line.len() - line.trim_start().len();

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

                for sel_part in selector_text.split(',') {
                    let sel = sel_part.trim();
                    if sel.is_empty() { continue; }

                    let sel_offset_in_line = if let Some(pos) = line.find(sel) {
                        pos
                    } else {
                        leading
                    };

                    flag_selectors_in_text(ctx, sel, base + byte_offset + sel_offset_in_line,
                        &check_class_selector, &check_type_selector, &check_id_selector, ELEMENT_SELECTORS);
                }
            }

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
                while pos < bytes.len() && bytes[pos] != b']' { pos += 1; }
                if pos < bytes.len() { pos += 1; }
            }
            b':' => {
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
