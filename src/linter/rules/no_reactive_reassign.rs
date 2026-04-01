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
        // Config: { "props": false } — skip checking property mutations on reactive vars
        let check_props = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("props"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

            // Single pass: collect declarations and reactive vars
            let mut reactive_vars = HashSet::new();
            let mut declared_vars = HashSet::new();

            for line in content.lines() {
                let trimmed = line.trim();
                // Collect let/var/const declarations (including export let/var/const)
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
                // Collect reactive declarations
                if trimmed.starts_with("$:") {
                    let after = trimmed[2..].trim_start();
                    if let Some(eq_pos) = after.find('=') {
                        let name = after[..eq_pos].trim();
                        if name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            && !name.is_empty()
                        {
                            // Will filter out pre-declared vars after the loop
                            reactive_vars.insert(name.to_string());
                        }
                    }
                }
            }

            // Remove pre-declared vars from reactive set
            reactive_vars.retain(|v| !declared_vars.contains(v));

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
                    format!("{} +=", var),
                    format!("{} -=", var),
                    format!("{} *=", var),
                    format!("{} /=", var),
                    format!("{} %=", var),
                    format!("{} &&=", var),
                    format!("{} ||=", var),
                    format!("{} ??=", var),
                ];
                for pattern in &patterns {
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(pattern.as_str()) {
                        let abs = search_from + pos;

                        // Skip if this is the reactive declaration itself ($: var = ...) or a comment
                        let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = content[line_start..].trim_start();
                        if line.starts_with("$:") || line.starts_with("//") || line.starts_with("/*") {
                            search_from = abs + pattern.len();
                            continue;
                        }

                        // Skip if preceded by alphanumeric, underscore, or dot
                        // (not a word boundary, or a member access like obj.name = ...)
                        if abs > 0 {
                            let prev = content.as_bytes()[abs - 1];
                            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'.' {
                                search_from = abs + pattern.len();
                                continue;
                            }
                        }

                        // Skip if inside brackets: `obj[reactiveVar] = ...`
                        // (computed property access, not assignment to reactiveVar)
                        if abs > 0 {
                            // Look backwards to check if this is inside [...] = context
                            let before = &content[..abs];
                            let last_bracket_open = before.rfind('[');
                            let last_bracket_close = before.rfind(']');
                            if let Some(bo) = last_bracket_open {
                                // If the last `[` is after the last `]`, we're inside brackets
                                let inside = match last_bracket_close {
                                    Some(bc) => bo > bc,
                                    None => true,
                                };
                                if inside {
                                    // Check that the `]` is right after our var name + whitespace
                                    let after_var = abs + var.len();
                                    let rest = content[after_var..].trim_start();
                                    if rest.starts_with(']') {
                                        search_from = abs + pattern.len();
                                        continue;
                                    }
                                }
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
                            format!("Assignment to reactive value '{}'.", var),
                            oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                        );
                        search_from = abs + pattern.len();
                    }
                }
            }

            // Step 2b: Check for mutating method calls on reactive vars (only if props checking enabled)
            if check_props {
            let mutating_methods = [
                "push(", "pop(", "shift(", "unshift(", "splice(",
                "sort(", "reverse(", "fill(", "copyWithin(",
            ];
            for var in &reactive_vars {
                for method in &mutating_methods {
                    let pattern = format!("{}.{}", var, method);
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(&pattern) {
                        let abs = search_from + pos;
                        // Word boundary check
                        if abs > 0 {
                            let prev = content.as_bytes()[abs - 1];
                            if prev.is_ascii_alphanumeric() || prev == b'_' {
                                search_from = abs + pattern.len();
                                continue;
                            }
                        }
                        // Skip if in $: declaration
                        let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = content[line_start..].trim_start();
                        if line.starts_with("$:") {
                            search_from = abs + pattern.len();
                            continue;
                        }
                        let source_pos = content_offset + abs;
                        ctx.diagnostic(
                            format!("Assignment to reactive value '{}'.", var),
                            oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                        );
                        search_from = abs + pattern.len();
                    }
                }
                // Also check member increment/decrement: var.prop++ var.prop-- and var?.prop++ var?.prop--
                for suffix in &["++", "--"] {
                    let mut search_from = 0;
                    while let Some(pos) = content[search_from..].find(&format!("{}.", var)) {
                        let abs = search_from + pos;
                        if abs > 0 {
                            let prev = content.as_bytes()[abs - 1];
                            if prev.is_ascii_alphanumeric() || prev == b'_' {
                                search_from = abs + var.len() + 1;
                                continue;
                            }
                        }
                        let after_dot = &content[abs + var.len() + 1..];
                        let prop_end = after_dot.find(|c: char| !c.is_alphanumeric() && c != '_')
                            .unwrap_or(after_dot.len());
                        let after_prop = &after_dot[prop_end..];
                        if after_prop.starts_with(suffix) {
                            let line_start = content[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
                            let line = content[line_start..].trim_start();
                            if !line.starts_with("$:") {
                                let source_pos = content_offset + abs;
                                let end_pos = source_pos + var.len() + 1 + prop_end + suffix.len();
                                ctx.diagnostic(
                                    format!("Assignment to property of reactive value '{}'.", var),
                                    oxc::span::Span::new(source_pos as u32, end_pos as u32),
                                );
                            }
                        }
                        search_from = abs + var.len() + 1;
                    }
                }
                // Also check member assignment: var.prop = and var[idx] = and var?.prop =
                for pattern_base in &[format!("{}.", var), format!("{}?.", var), format!("{}[", var)] {
                    for (pos, _) in content.match_indices(pattern_base.as_str()) {
                        if pos > 0 {
                            let prev = content.as_bytes()[pos - 1];
                            if prev.is_ascii_alphanumeric() || prev == b'_' { continue; }
                        }
                        let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = content[line_start..].trim_start();
                        if line.starts_with("$:") { continue; }

                        let after = &content[pos + pattern_base.len()..];
                        // Consume initial member access, then follow chained .prop and [idx]
                        let mut rest = if pattern_base.ends_with('[') {
                            after.find(']').map(|p| &after[p+1..]).unwrap_or("")
                        } else {
                            let end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
                            &after[end..]
                        };
                        // Follow chained property/index access: .prop, [idx], ?.prop
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
                            let source_pos = content_offset + pos;
                            ctx.diagnostic(
                                format!("Assignment to property of reactive value '{}'.", var),
                                oxc::span::Span::new(source_pos as u32, (source_pos + pattern_base.len()) as u32),
                            );
                        }
                    }
                }
                // Check for delete var.prop
                let delete_pattern = format!("delete {}", var);
                for (pos, _) in content.match_indices(&delete_pattern) {
                    let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let line = content[line_start..].trim_start();
                    if line.starts_with("$:") { continue; }
                    let source_pos = content_offset + pos;
                    ctx.diagnostic(
                        format!("Assignment to property of reactive value '{}'.", var),
                        oxc::span::Span::new(source_pos as u32, (source_pos + delete_pattern.len()) as u32),
                    );
                }
            }
            } // end if check_props (step 2b)

            // Step 2c: Check for destructuring assignments targeting reactive vars
            for var in &reactive_vars {
                let destructure_patterns = [
                    format!("{} }} =", var),     // { foo: reactiveVar } =
                    format!("{}}} =", var),      // {reactiveVar} = (no space)
                    format!("{}] =", var),       // [reactiveVar] =
                    format!("...{} }} =", var),  // { ...reactiveVar } =
                    format!("...{}] =", var),    // [...reactiveVar] =
                ];
                for pattern in &destructure_patterns {
                    if let Some(pos) = content.find(pattern.as_str()) {
                        let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let line = content[line_start..].trim_start();
                        if line.starts_with("$:") || line.starts_with("const ")
                            || line.starts_with("let ") || line.starts_with("var ") { continue; }

                        // For `reactiveVar] =` patterns, verify this is actually a
                        // destructuring `[reactiveVar] =`, not computed property
                        // access `obj[reactiveVar] = value`.
                        if pattern.ends_with("] =") && !pattern.starts_with("...") {
                            // Find the matching `[` before the var name
                            let before = &content[..pos];
                            if let Some(bracket_pos) = before.rfind('[') {
                                let between = content[bracket_pos + 1..pos].trim();
                                // In true destructuring, nothing (or whitespace) is between `[` and the var
                                // In computed access, the `[` is preceded by an identifier/expression
                                if between.is_empty() {
                                    // Check if char before `[` indicates destructuring
                                    let before_bracket = content[..bracket_pos].trim_end();
                                    if !(before_bracket.ends_with('=')
                                        || before_bracket.ends_with(',')
                                        || before_bracket.ends_with(';')
                                        || before_bracket.ends_with('{')
                                        || before_bracket.ends_with('(')
                                        || before_bracket.is_empty()
                                        || before_bracket.ends_with('\n'))
                                    {
                                        // Preceded by an expression — this is computed property access
                                        continue;
                                    }
                                } else {
                                    // Something between `[` and var — likely `[other, reactiveVar]`
                                    // which is destructuring — let it through
                                }
                            }
                        }

                        let source_pos = content_offset + pos;
                        ctx.diagnostic(
                            format!("Assignment to reactive value '{}'.", var),
                            oxc::span::Span::new(source_pos as u32, (source_pos + var.len()) as u32),
                        );
                        break; // Only report once per var per pattern type
                    }
                }
            }

            // Step 2d: Check for for-of/for-in assignment to reactive var members
            for var in &reactive_vars {
                let for_patterns = [
                    format!("for ({} ", var),
                    format!("for ({}", var),
                    format!("for (const {} ", var),
                    format!("for (let {} ", var),
                ];
                // Also check for (reactiveValue.key of/in ...)
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
                            let source_pos = content_offset + pos;
                            ctx.diagnostic(
                                format!("Assignment to property of reactive value '{}'.", var),
                                oxc::span::Span::new(source_pos as u32, (source_pos + pattern.len()) as u32),
                            );
                        }
                    }
                }
            }

            // Step 2d: Check for conditional member assignment: (x ? reactiveVar : y).prop =
            if check_props { for var in &reactive_vars {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("$:") { continue; }
                    // Pattern: (... reactiveVar ...).prop = value
                    if trimmed.contains(&format!("? {} :", var))
                        || trimmed.contains(&format!("? {}", var))
                    {
                        // Check if the line has a member assignment
                        if let Some(dot_pos) = trimmed.rfind(").") {
                            let after_dot = &trimmed[dot_pos + 2..];
                            let end = after_dot.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after_dot.len());
                            let rest = after_dot[end..].trim_start();
                            if rest.starts_with('=') && !rest.starts_with("==") {
                                if let Some(pos) = content.find(trimmed) {
                                    let source_pos = content_offset + pos;
                                    ctx.diagnostic(
                                        format!("Assignment to property of reactive value '{}'.", var),
                                        oxc::span::Span::new(source_pos as u32, (source_pos + trimmed.len()) as u32),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            } // end if check_props (conditional member assignment)

            // Step 3: Check template for event handlers, expression props, and bind: directives
            walk_template_nodes(&ctx.ast.html, &mut |node| {
                if let TemplateNode::Element(el) = node {
                    for attr in &el.attributes {
                        // Collect spans of expression regions to check for assignments.
                        // This covers event handlers (on:click), expression attributes
                        // (prop={() => { ... }}), etc.
                        let expr_span = match attr {
                            Attribute::Directive { kind: DirectiveKind::EventHandler, span, .. } => Some(*span),
                            Attribute::NormalAttribute { span, value, .. } => {
                                // Only check attributes with expression values
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
                            for var in &reactive_vars {
                                // Check simple and compound assignments
                                let pats = [
                                    format!("{} = ", var),
                                    format!("{} += ", var),
                                    format!("{} -= ", var),
                                    format!("{} *= ", var),
                                    format!("{} /= ", var),
                                    format!("{} %= ", var),
                                    format!("{}++", var),
                                    format!("{}--", var),
                                ];
                                'next_var: for pat in &pats {
                                    for (pos, _) in region.match_indices(pat.as_str()) {
                                        if pos > 0 {
                                            let prev = region.as_bytes()[pos - 1];
                                            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev == b'.' { continue; }
                                        }
                                        // Skip == (comparison)
                                        if pat.ends_with("= ") {
                                            let eq_pos = pos + pat.len() - 1;
                                            if eq_pos < region.len() && region.as_bytes()[eq_pos] == b'=' { continue; }
                                        }
                                        let before = &region[..pos];
                                        let single_quotes = before.matches('\'').count();
                                        let double_quotes = before.matches('"').count();
                                        if single_quotes % 2 != 0 || double_quotes % 2 != 0 { continue; }

                                        let abs_pos = span.start as usize + pos;
                                        ctx.diagnostic(
                                            format!("Assignment to reactive value '{}'.", var),
                                            oxc::span::Span::new(abs_pos as u32, (abs_pos + var.len()) as u32),
                                        );
                                        break 'next_var;
                                    }
                                }
                            }
                            // Check event handlers for property assignments to reactive
                            // vars and their store subscriptions ($varName).
                            // E.g.: $gallery.items[idx] = value
                            if check_props {
                                for var in &reactive_vars {
                                    // Check both var.prop and $var.prop patterns
                                    for prefix in &[var.clone(), format!("${}", var)] {
                                        for pat_start in &[format!("{}.", prefix), format!("{}[", prefix)] {
                                            for (pos, _) in region.match_indices(pat_start.as_str()) {
                                                // Word boundary check
                                                if pos > 0 {
                                                    let prev = region.as_bytes()[pos - 1];
                                                    if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' { continue; }
                                                }
                                                // String context check
                                                let before = &region[..pos];
                                                let single_quotes = before.matches('\'').count();
                                                let double_quotes = before.matches('"').count();
                                                if single_quotes % 2 != 0 || double_quotes % 2 != 0 { continue; }

                                                // Follow member chain to find = assignment
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
                                                    let abs_pos = span.start as usize + pos;
                                                    ctx.diagnostic(
                                                        format!("Assignment to property of reactive value '{}'.", prefix),
                                                        oxc::span::Span::new(abs_pos as u32, (abs_pos + pat_start.len()) as u32),
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Attribute::Directive { kind: DirectiveKind::Binding, name, span, .. } = attr {
                            let region = &ctx.source[span.start as usize..span.end as usize];
                            if let Some(open) = region.find('{') {
                                if let Some(close) = region.find('}') {
                                    let bound_var = region[open+1..close].trim();
                                    // Check both direct var and var.member
                                    let base_var = bound_var.split('.').next().unwrap_or(bound_var);
                                    let is_member = bound_var.contains('.');
                                    if reactive_vars.contains(bound_var) || (reactive_vars.contains(base_var) && (check_props || !is_member)) {
                                        ctx.diagnostic(
                                            format!("Assignment to reactive value '{}'.", base_var),
                                            *span,
                                        );
                                    }
                                }
                            } else if reactive_vars.contains(name.as_str()) {
                                // Shorthand binding: bind:varName (no expression)
                                ctx.diagnostic(
                                    format!("Assignment to reactive value '{}'.", name),
                                    *span,
                                );
                            }
                        }
                    }
                }
            });
        }
    }
}
