//! `svelte/no-reactive-reassign` — disallow reassignment of reactive values.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};
use std::collections::HashSet;

pub struct NoReactiveReassign;

impl Rule for NoReactiveReassign {
    fn name(&self) -> &'static str {
        "svelte/no-reactive-reassign"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

            // Step 1: Find reactive variable names (declared with $: name = ...)
            // Only flag variables that are NOT pre-declared with let/var/const
            let mut reactive_vars = HashSet::new();
            let mut declared_vars = HashSet::new();

            // First pass: collect let/var/const declarations
            for line in content.lines() {
                let trimmed = line.trim();
                for kw in &["let ", "var ", "const "] {
                    if trimmed.starts_with(kw) {
                        let rest = trimmed[kw.len()..].trim_start();
                        let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                            .unwrap_or(rest.len());
                        let name = &rest[..name_end];
                        if !name.is_empty() {
                            declared_vars.insert(name.to_string());
                        }
                    }
                }
            }

            // Second pass: find reactive declarations
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("$:") {
                    let after = trimmed[2..].trim_start();
                    if let Some(eq_pos) = after.find('=') {
                        let before_eq = after[..eq_pos].trim();
                        if before_eq.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            && !before_eq.is_empty()
                            && !declared_vars.contains(before_eq)
                        {
                            reactive_vars.insert(before_eq.to_string());
                        }
                    }
                }
            }

            if reactive_vars.is_empty() { return; }

            // Step 2: Look for reassignments of reactive vars inside function bodies
            // Find function/handler bodies and check for reactiveVar = or reactiveVar++/--
            for var in &reactive_vars {
                // Look for assignments: var = (not inside $: declarations)
                let patterns = [
                    format!("{} =", var),
                    format!("{}=", var),
                    format!("{}++", var),
                    format!("{}--", var),
                ];
                for pattern in &patterns {
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(pattern.as_str()) {
                        let abs = search_from + pos;

                        // Skip if this is the reactive declaration itself ($: var = ...)
                        let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = content[line_start..].trim_start();
                        if line.starts_with("$:") {
                            search_from = abs + pattern.len();
                            continue;
                        }

                        // Skip if preceded by alphanumeric (not a word boundary)
                        if abs > 0 {
                            let prev = content.as_bytes()[abs - 1];
                            if prev.is_ascii_alphanumeric() || prev == b'_' {
                                search_from = abs + pattern.len();
                                continue;
                            }
                        }

                        // Skip == (comparison, not assignment)
                        if pattern.ends_with(" =") || pattern.ends_with('=') {
                            let after_eq = abs + pattern.len();
                            if after_eq < content.len() && content.as_bytes()[after_eq] == b'=' {
                                search_from = abs + pattern.len();
                                continue;
                            }
                        }

                        let source_pos = content_offset + abs;
                        ctx.diagnostic(
                            format!("Do not reassign the reactive variable `{}`. It is derived from a reactive declaration.", var),
                            oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                        );
                        search_from = abs + pattern.len();
                    }
                }
            }

            // Step 3: Check template for bind: directives on reactive vars
            walk_template_nodes(&ctx.ast.html, &mut |node| {
                if let TemplateNode::Element(el) = node {
                    for attr in &el.attributes {
                        if let Attribute::Directive { kind: DirectiveKind::Binding, name, span, .. } = attr {
                            if name == "value" || name == "checked" || name == "group" {
                                let region = &ctx.source[span.start as usize..span.end as usize];
                                if let Some(open) = region.find('{') {
                                    if let Some(close) = region.find('}') {
                                        let bound_var = region[open+1..close].trim();
                                        if reactive_vars.contains(bound_var) {
                                            ctx.diagnostic(
                                                format!("Do not bind to the reactive variable `{}`. It is derived from a reactive declaration.", bound_var),
                                                *span,
                                            );
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
}
