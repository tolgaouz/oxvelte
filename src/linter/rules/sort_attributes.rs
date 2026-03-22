//! `svelte/sort-attributes` — enforce attribute sorting order.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute};

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
                // Split attributes into groups separated by spread attributes
                let mut groups: Vec<Vec<&str>> = vec![vec![]];
                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name, .. } => {
                            groups.last_mut().unwrap().push(name.as_str());
                        }
                        Attribute::Directive { name, .. } => {
                            groups.last_mut().unwrap().push(name.as_str());
                        }
                        Attribute::Spread { .. } => {
                            // Start a new group after spread
                            groups.push(vec![]);
                        }
                    }
                }
                let names = groups.clone().into_iter().flatten().collect::<Vec<_>>();
                let _ = names; // use groups instead

                for group in &groups {
                    if order_rules.is_empty() {
                        // Default: alphabetical sorting
                        for window in group.windows(2) {
                            if window[0].to_lowercase() > window[1].to_lowercase() {
                                ctx.diagnostic(
                                    format!("Attributes should be sorted. `{}` should come before `{}`.", window[1], window[0]),
                                    el.span,
                                );
                                break;
                            }
                        }
                    } else {
                        for rule in &order_rules {
                            let matched: Vec<&str> = group.iter()
                                .filter(|n| matches_pattern(n, &rule.patterns))
                                .copied()
                                .collect();
                            if rule.sort == "alphabetical" {
                                for window in matched.windows(2) {
                                    if window[0].to_lowercase() > window[1].to_lowercase() {
                                        ctx.diagnostic(
                                            format!("Attributes should be sorted. `{}` should come before `{}`.", window[1], window[0]),
                                            el.span,
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
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

fn matches_pattern(name: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        // Handle regex patterns: "/^prefix-/u"
        if pattern.starts_with('/') {
            let inner = pattern.trim_start_matches('/');
            let inner = inner.rsplit_once('/').map(|(p, _)| p).unwrap_or(inner);
            // Simple prefix matching for ^prefix patterns
            if inner.starts_with('^') {
                let prefix = &inner[1..].replace("-/", "-");
                if name.starts_with(prefix) { return true; }
            }
        } else if pattern == name {
            return true;
        }
    }
    false
}
