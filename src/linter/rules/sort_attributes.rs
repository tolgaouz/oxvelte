//! `svelte/sort-attributes` — enforce attribute sorting order.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct SortAttributes;

impl Rule for SortAttributes {
    fn name(&self) -> &'static str {
        "svelte/sort-attributes"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let order_rules = parse_order_config(&ctx.config.options);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let mut groups: Vec<Vec<String>> = vec![vec![]];
                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name, .. } => {
                            groups.last_mut().unwrap().push(name.clone());
                        }
                        Attribute::Directive { kind, name, .. } => {
                            let prefix = directive_prefix(kind);
                            let full_name = format!("{}:{}", prefix, name);
                            groups.last_mut().unwrap().push(full_name);
                        }
                        Attribute::Spread { .. } => {
                            groups.push(vec![]);
                        }
                    }
                }

                let msg = |a: &str, b: &str| format!("Attribute '{}' should go before '{}'.", a, b);
                for group in &groups {
                    let mut seen = std::collections::HashSet::new();
                    if group.iter().any(|n| !seen.insert(n.as_str())) { continue; }

                    if order_rules.is_empty() {
                        fn cat(n: &str) -> u8 {
                            if n == "this" || n == "bind:this" { return 0; }
                            if n == "slot" || n == "name" || n == "id" { return 1; }
                            if n.starts_with("class:") || n.starts_with("style:") { return 2; }
                            if n.starts_with("bind:") || n.starts_with("on:") { return 3; }
                            if n.starts_with("use:") { return 4; }
                            if n.starts_with("transition:") { return 5; }
                            if n.starts_with("in:") || n.starts_with("out:") { return 6; }
                            if n.starts_with("animate:") { return 7; }
                            if n.starts_with("let:") { return 8; }
                            2
                        }
                        fn sub_cat(n: &str) -> u8 {
                            if n.starts_with("bind:") { 0 } else if n.starts_with("on:") { 1 }
                            else if n == "style" { 2 } else if n.starts_with("style:") { 3 }
                            else if n == "class" { 4 } else if n.starts_with("class:") { 5 }
                            else { 0 }
                        }
                        for w in group.windows(2) {
                            let (c0, c1) = (cat(&w[0]), cat(&w[1]));
                            if c0 > c1 { ctx.diagnostic(msg(&w[1], &w[0]), el.span); }
                            if c0 == c1 && sub_cat(&w[0]) == sub_cat(&w[1])
                                && !(w[0].starts_with("style:") && w[1].starts_with("style:"))
                                && w[0].to_lowercase() > w[1].to_lowercase() {
                                ctx.diagnostic(msg(&w[1], &w[0]), el.span);
                            }
                        }
                    } else {
                        let positions: Vec<Option<usize>> = group.iter().map(|n| find_order_position(n, &order_rules)).collect();
                        let (mut last_pos, mut last_idx) = (None, 0);
                        for i in 0..group.len() {
                            if let Some(pos) = positions[i] {
                                if last_pos.map_or(false, |p| pos < p) { ctx.diagnostic(msg(&group[i], &group[last_idx]), el.span); }
                                last_pos = Some(pos);
                                last_idx = i;
                            }
                        }
                        for rule in &order_rules {
                            if rule.sort != "alphabetical" { continue; }
                            let matched: Vec<&str> = group.iter().filter(|n| matches_pattern(n, &rule.patterns)).map(|s| s.as_str()).collect();
                            for w in matched.windows(2) {
                                if w[0].to_lowercase() > w[1].to_lowercase() { ctx.diagnostic(msg(w[1], w[0]), el.span); }
                            }
                        }
                    }
                }
            }
        });
    }
}

fn directive_prefix(kind: &DirectiveKind) -> &'static str {
    match kind {
        DirectiveKind::EventHandler => "on",
        DirectiveKind::Binding => "bind",
        DirectiveKind::Class => "class",
        DirectiveKind::StyleDirective => "style",
        DirectiveKind::Use => "use",
        DirectiveKind::Transition => "transition",
        DirectiveKind::In => "in",
        DirectiveKind::Out => "out",
        DirectiveKind::Animate => "animate",
        DirectiveKind::Let => "let",
    }
}

struct OrderRule {
    patterns: Vec<String>,
    sort: String,
}

fn parse_order_config(options: &Option<serde_json::Value>) -> Vec<OrderRule> {
    let Some(order) = options.as_ref().and_then(|v| v.as_array()).and_then(|a| a.first())
        .and_then(|o| o.get("order")).and_then(|o| o.as_array()) else { return vec![]; };
    order.iter().filter_map(|entry| match entry {
        serde_json::Value::String(p) => Some(OrderRule { patterns: vec![p.clone()], sort: "ignore".to_string() }),
        serde_json::Value::Array(arr) => {
            let pats: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            Some(OrderRule { patterns: pats, sort: "ignore".to_string() })
        }
        serde_json::Value::Object(obj) => {
            let pats = obj.get("match").and_then(|m| m.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            Some(OrderRule { patterns: pats, sort: obj.get("sort").and_then(|s| s.as_str()).unwrap_or("alphabetical").to_string() })
        }
        _ => None,
    }).collect()
}

fn find_order_position(name: &str, rules: &[OrderRule]) -> Option<usize> {
    rules.iter().position(|r| matches_pattern(name, &r.patterns))
}

fn matches_pattern(name: &str, patterns: &[String]) -> bool {
    if patterns.iter().any(|p| p.starts_with('!') && matches_single_pattern(name, &p[1..])) { return false; }
    patterns.iter().any(|p| !p.starts_with('!') && matches_single_pattern(name, p))
}

fn matches_single_pattern(name: &str, pattern: &str) -> bool {
    if !pattern.starts_with('/') { return pattern == name; }
    let inner = pattern.trim_start_matches('/');
    let inner = inner.rsplit_once('/').map(|(p, _)| p).unwrap_or(inner);
    if inner.starts_with('^') {
        let prefix = inner[1..].trim_end_matches('$');
        if prefix.starts_with("(?:") || prefix.contains('|') || prefix.contains('[') {
            return inner.strip_prefix("^(?:").and_then(|r| r.strip_suffix(")$"))
                .map_or(false, |alts| alts.split('|').any(|a| a == name));
        }
        return name.starts_with(prefix);
    }
    if inner == ":" || inner == ":/u" { return name.contains(':'); }
    if inner.starts_with('!') {
        let check = &inner[1..];
        return if check == ":" { !name.contains(':') } else { !name.starts_with(check) };
    }
    false
}
