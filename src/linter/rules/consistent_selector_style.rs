//! `svelte/consistent-selector-style` — enforce consistent style selector usage
//! (e.g. prefer class selectors over element selectors).
//!
//! Uses `crate::parser::selector::for_each_rule_prelude` (a cssparser
//! `StyleSheetParser` driver) to iterate every rule's prelude — including
//! those nested in `@media`, `@supports`, and Svelte's `:global { … }`
//! wrapper. Each prelude is parsed into a typed `SelectorList`, and
//! `Component::Class` / `Component::ID` / `Component::LocalName` are
//! dispatched to per-kind preference checks. No brace tracking, no string
//! scanning for selector fragments.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue};
use crate::parser::selector::{for_each_rule_prelude, parse_selector_list, walk_components};
use oxc::span::Span;
use selectors::parser::Component;
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
    walk_template_nodes(html, &mut |node| if let TemplateNode::Element(el) = node {
        found |= el.attributes.iter().any(|a| matches!(a,
            Attribute::Directive { kind: crate::ast::DirectiveKind::Class, name, .. } if name == class_name));
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

        // Early exit: if there's nothing to check and no explicit style
        // preferences, this rule is a no-op.
        if allowed_styles.is_empty() && !check_global {
            return;
        }

        let id_pos = allowed_styles.iter().position(|s| s == "id")
            .or(if allowed_styles.is_empty() { Some(1) } else { None });
        let class_pos = allowed_styles.iter().position(|s| s == "class")
            .or(if allowed_styles.is_empty() { Some(2) } else { None });
        let type_pos = allowed_styles.iter().position(|s| s == "type")
            .or(if allowed_styles.is_empty() { Some(0) } else { None });

        let flag_class = !allowed_styles.is_empty() || check_global;

        let (class_usage, id_usage, element_usage) = collect_template_info(&ctx.ast.html);
        let html = &ctx.ast.html;

        // Preference-check closures — unchanged from the byte-scanner era.
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

            if can_be_id && id_pos.zip(class_pos).is_some_and(|(ip, cp)| ip < cp) {
                return Some("ID".to_string());
            }
            if can_be_type && type_pos.zip(class_pos).is_some_and(|(tp, cp)| tp < cp) {
                return Some("element type".to_string());
            }
            None
        };

        let check_type_selector = |elem_name: &str| -> Option<String> {
            if element_usage.get(elem_name).copied().unwrap_or(0) <= 1
                && id_pos.zip(type_pos).is_some_and(|(ip, tp)| ip < tp) {
                return Some("ID".to_string());
            }
            None
        };

        let check_id_selector = |id_name: &str| -> Option<String> {
            if let Some(et) = id_usage.get(id_name) {
                if element_usage.get(et).copied().unwrap_or(0) <= 1
                    && type_pos.zip(id_pos).is_some_and(|(tp, ip)| tp < ip) {
                    return Some("element type".to_string());
                }
            }
            None
        };

        // Walk every rule's prelude via cssparser's StyleSheetParser.
        // `inside_global` is true for rules nested inside `:global { … }`;
        // `walk_components(visit_global=check_global)` additionally descends
        // into `:global(...)` inline pseudo-class bodies.
        let mut diagnostics: Vec<(String, Span)> = Vec::new();
        let style_span_start = style.span.start;
        let css_start_in_source = style_span_start
            + (ctx.source[style_span_start as usize..style.span.end as usize]
                .find('>').map(|p| p + 1).unwrap_or(0)) as u32;

        for_each_rule_prelude(&style.content, css_start_in_source, |prelude_text, start_in_source, inside_global| {
            // When inside `:global { … }`, only emit if the user opted in.
            if inside_global && !check_global { return; }

            let Some(list) = parse_selector_list(prelude_text) else { return };
            walk_components(&list, check_global, &mut |comp, in_global_body| {
                // `in_global_body` marks components that live inside
                // `:global(...)` inline pseudo — skip when not opted in.
                if in_global_body && !check_global { return; }
                match comp {
                    Component::Class(atom) => {
                        let name = atom.as_str();
                        if let Some(suggested) = check_class_selector(name) {
                            let (sp, end) = locate_prefixed(prelude_text, '.', name, start_in_source as usize);
                            diagnostics.push((
                                format!("Selector should select by {} instead of class", suggested),
                                Span::new(sp as u32, end as u32),
                            ));
                        }
                    }
                    Component::ID(atom) => {
                        let name = atom.as_str();
                        if let Some(suggested) = check_id_selector(name) {
                            let (sp, end) = locate_prefixed(prelude_text, '#', name, start_in_source as usize);
                            diagnostics.push((
                                format!("Selector should select by {} instead of ID", suggested),
                                Span::new(sp as u32, end as u32),
                            ));
                        }
                    }
                    Component::LocalName(local) => {
                        let name = local.name.as_str();
                        if let Some(suggested) = check_type_selector(name) {
                            let (sp, end) = locate_tag(prelude_text, name, start_in_source as usize);
                            diagnostics.push((
                                format!("Selector should select by {} instead of element type", suggested),
                                Span::new(sp as u32, end as u32),
                            ));
                        }
                    }
                    _ => {}
                }
            });
        });

        for (msg, span) in diagnostics {
            ctx.diagnostic(msg, span);
        }
    }
}

/// Find `.name` / `#name` in `text` and return its absolute (start, end) in
/// the source. Falls back to the prelude start if the prefix isn't found
/// (shouldn't happen for a selector the parser accepted).
fn locate_prefixed(text: &str, prefix: char, name: &str, base_offset: usize) -> (usize, usize) {
    let needle = format!("{}{}", prefix, name);
    let rel = text.find(&needle).unwrap_or(0);
    let start = base_offset + rel;
    (start, start + needle.len())
}

/// Find a bare tag name in a selector (not preceded/followed by another
/// identifier character) and return its absolute (start, end) in source.
fn locate_tag(text: &str, name: &str, base_offset: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let name_len = name.len();
    let mut search = 0;
    while let Some(rel) = text[search..].find(name) {
        let abs_rel = search + rel;
        let before = if abs_rel == 0 { 0u8 } else { bytes[abs_rel - 1] };
        let after_idx = abs_rel + name_len;
        let after = if after_idx < bytes.len() { bytes[after_idx] } else { 0u8 };
        let bad_before = before.is_ascii_alphanumeric()
            || matches!(before, b'_' | b'-' | b'.' | b'#' | b':' | b'[');
        let bad_after = after.is_ascii_alphanumeric() || matches!(after, b'_' | b'-');
        if !bad_before && !bad_after {
            let start = base_offset + abs_rel;
            return (start, start + name_len);
        }
        search = abs_rel + name_len;
    }
    (base_offset, base_offset + name_len)
}
