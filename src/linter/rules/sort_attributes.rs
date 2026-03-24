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
        // Parse order config
        let order_rules = parse_order_config(&ctx.config.options);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                // Build full attribute names including directive prefixes
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
                            // Start a new group after spread
                            groups.push(vec![]);
                        }
                    }
                }

                for group in &groups {
                    // Skip groups with duplicate names (semantic ordering intended)
                    let has_duplicates = {
                        let mut seen = std::collections::HashSet::new();
                        group.iter().any(|n| !seen.insert(n.as_str()))
                    };
                    if has_duplicates {
                        continue;
                    }

                    if order_rules.is_empty() {
                        // Default: alphabetical sorting, but only compare within
                        // the same attribute category.
                        fn attr_category(name: &str) -> u8 {
                            // Category 0: this, bind:this (always first)
                            if name == "this" || name == "bind:this" { return 0; }
                            // Category 1: slot, name, id (before normal attrs, after this)
                            if name == "slot" || name == "name" || name == "id" { return 1; }
                            // Category 2: normal attrs + id + name + class: + style: directives
                            if name.starts_with("class:") || name.starts_with("style:") { return 2; }
                            // Category 3: bind: / on: directives
                            if name.starts_with("bind:") || name.starts_with("on:") { return 3; }
                            // Category 4: use: directives
                            if name.starts_with("use:") { return 4; }
                            // Category 5: transition:
                            if name.starts_with("transition:") { return 5; }
                            // Category 6: in: / out: (sorted alphabetically within)
                            if name.starts_with("in:") || name.starts_with("out:") { return 6; }
                            // Category 7: animate:
                            if name.starts_with("animate:") { return 7; }
                            // Category 8: let:
                            if name.starts_with("let:") { return 8; }
                            // Category 2: normal attributes
                            2
                        }

                        // Sub-category for within-group comparison.
                        // Items with different sub-categories in the same category are NOT compared.
                        fn attr_sub_category(name: &str) -> u8 {
                            // bind: and on: are not compared to each other
                            if name.starts_with("bind:") { return 0; }
                            if name.starts_with("on:") { return 1; }
                            // style (attr) is separate from style: directives
                            if name == "style" { return 2; }
                            // style: directives are not sorted among themselves
                            if name.starts_with("style:") { return 3; }
                            // class (attr) is separate from class: directives
                            if name == "class" { return 4; }
                            // class: directives sorted among themselves
                            if name.starts_with("class:") { return 5; }
                            // Other normal attrs sorted among themselves
                            0
                        }

                        for window in group.windows(2) {
                            let cat0 = attr_category(&window[0]);
                            let cat1 = attr_category(&window[1]);
                            // Enforce category order: lower category must come first
                            if cat0 > cat1 {
                                ctx.diagnostic(
                                    format!("Attribute '{}' should go before '{}'.", window[1], window[0]),
                                    el.span,
                                );
                            }
                            // Within the same category, enforce alphabetical order
                            // But skip comparison between different sub-categories (e.g. bind: vs on:)
                            // And skip style: directives (they can be in any order)
                            if cat0 == cat1 {
                                let sub0 = attr_sub_category(&window[0]);
                                let sub1 = attr_sub_category(&window[1]);
                                // Skip sorting for style: directives
                                let both_style_dir = window[0].starts_with("style:") && window[1].starts_with("style:");
                                if sub0 == sub1 && !both_style_dir && window[0].to_lowercase() > window[1].to_lowercase() {
                                    ctx.diagnostic(
                                        format!("Attribute '{}' should go before '{}'.", window[1], window[0]),
                                        el.span,
                                    );
                                }
                            }
                        }
                    } else {
                        // Custom order: assign each attribute a position based on which
                        // order rule it matches first. None means unmatched (free position).
                        let positions: Vec<Option<usize>> = group.iter()
                            .map(|name| find_order_position(name, &order_rules))
                            .collect();

                        // Check that positions are non-decreasing (skip unmatched)
                        // Track the last matched position to compare against
                        let mut last_matched_pos = None;
                        let mut last_matched_idx = 0;
                        for i in 0..group.len() {
                            if let Some(pos) = positions[i] {
                                if let Some(prev) = last_matched_pos {
                                    if pos < prev {
                                        ctx.diagnostic(
                                            format!("Attribute '{}' should go before '{}'.", group[i], group[last_matched_idx]),
                                            el.span,
                                        );
                                    }
                                }
                                last_matched_pos = Some(pos);
                                last_matched_idx = i;
                            }
                        }

                        // Also check alphabetical sorting within same-position groups
                        // that have sort == "alphabetical"
                        for rule in &order_rules {
                            if rule.sort != "alphabetical" { continue; }
                            let matched: Vec<&str> = group.iter()
                                .filter(|n| matches_order_rule(n, rule))
                                .map(|s| s.as_str())
                                .collect();
                            for window in matched.windows(2) {
                                if window[0].to_lowercase() > window[1].to_lowercase() {
                                    ctx.diagnostic(
                                        format!("Attribute '{}' should go before '{}'.", window[1], window[0]),
                                        el.span,
                                    );
                                }
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
    let mut rules = Vec::new();
    let opts = match options {
        Some(serde_json::Value::Array(arr)) => arr.first(),
        _ => return rules,
    };
    let opts = match opts {
        Some(o) => o,
        None => return rules,
    };
    let order = match opts.get("order").and_then(|o| o.as_array()) {
        Some(a) => a,
        None => return rules,
    };

    for entry in order {
        match entry {
            serde_json::Value::String(pattern) => {
                // Plain string pattern: no internal sorting
                rules.push(OrderRule {
                    patterns: vec![pattern.clone()],
                    sort: "ignore".to_string(),
                });
            }
            serde_json::Value::Array(arr) => {
                // Array of patterns: all patterns match to this position
                let patterns: Vec<String> = arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                rules.push(OrderRule {
                    patterns,
                    sort: "ignore".to_string(),
                });
            }
            serde_json::Value::Object(obj) => {
                let patterns = obj.get("match")
                    .and_then(|m| m.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let sort = obj.get("sort").and_then(|s| s.as_str()).unwrap_or("alphabetical").to_string();
                rules.push(OrderRule { patterns, sort });
            }
            _ => {}
        }
    }
    rules
}

/// Find the position (index) of the first order rule that matches the attribute name.
/// Returns None if no rule matches (unmatched attributes are free-positioned).
fn find_order_position(name: &str, rules: &[OrderRule]) -> Option<usize> {
    for (i, rule) in rules.iter().enumerate() {
        if matches_order_rule(name, rule) {
            return Some(i);
        }
    }
    None
}

/// Check if a name matches a specific order rule.
fn matches_order_rule(name: &str, rule: &OrderRule) -> bool {
    matches_pattern(name, &rule.patterns)
}

fn matches_pattern(name: &str, patterns: &[String]) -> bool {
    // Check for negative patterns first
    for pattern in patterns {
        if pattern.starts_with('!') {
            let neg = &pattern[1..];
            if matches_single_pattern(name, neg) {
                return false;
            }
        }
    }
    // Check positive patterns
    for pattern in patterns {
        if pattern.starts_with('!') { continue; }
        if matches_single_pattern(name, pattern) {
            return true;
        }
    }
    false
}

fn matches_single_pattern(name: &str, pattern: &str) -> bool {
    // Handle regex patterns: "/^prefix-/u" or "/pattern/u"
    if pattern.starts_with('/') {
        let inner = pattern.trim_start_matches('/');
        let inner = inner.rsplit_once('/').map(|(p, _)| p).unwrap_or(inner);

        if inner.starts_with('^') {
            // Prefix match
            let prefix = inner[1..].trim_end_matches('$');
            // Handle character class negation like !/:/u
            if prefix.starts_with("(?:") || prefix.contains('|') || prefix.contains('[') {
                // More complex regex - try simple cases
                if let Some(rest) = inner.strip_prefix("^(?:") {
                    // ^(?:id|class|value|src|style)$ pattern
                    if let Some(alts) = rest.strip_suffix(")$") {
                        return alts.split('|').any(|alt| alt == name);
                    }
                }
                return false;
            }
            return name.starts_with(prefix);
        }
        if inner == ":" || inner == ":/u" {
            // Match anything with a colon
            return name.contains(':');
        }
        // Negative lookahead-like patterns
        if inner.starts_with("!") {
            // Pattern like !/:/u means "does not contain :"
            let check = &inner[1..];
            if check == ":" { return !name.contains(':'); }
            return !name.starts_with(check);
        }
        return false;
    }

    // Exact match
    pattern == name
}
