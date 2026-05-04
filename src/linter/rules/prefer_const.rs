//! `svelte/prefer-const` — require `const` declarations for variables that are
//! never reassigned. 🔧 Fixable.
//!
//! Mirrors ESLint core's `prefer-const`, with the Svelte-specific override:
//! a declaration whose initializer is one of the configured `excludedRunes`
//! (`$props`, `$derived` by default) is skipped *as a whole declaration*,
//! not per-symbol.
//!
//! Implementation notes:
//! - Walks `AstKind::VariableDeclaration` so we can reason about every
//!   declarator in a single `let`.
//! - Symbol resolution uses `BindingIdentifier::symbol_id`. References are
//!   read off the scope-manager's `get_resolved_references`.
//! - The autofix replaces the `let` keyword with `const`, but only when *every*
//!   binding in the whole declaration is eligible (matching ESLint core).
//! - `ignoreReadBeforeAssign` is currently a no-op — implementing it
//!   faithfully needs read/write-ordering analysis we don't have yet.

use crate::linter::{Fix, LintContext, Rule};
use oxc::ast::ast::{BindingPattern, Expression, VariableDeclarationKind};
use oxc::ast::AstKind;
use oxc::semantic::{Semantic, SymbolId};
use oxc::span::Span;

pub struct PreferConst;

#[derive(Clone)]
struct Finding {
    message: String,
    span: Span,
    fix: Option<Fix>,
}

impl Rule for PreferConst {
    fn name(&self) -> &'static str {
        "svelte/prefer-const"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let excluded_runes: Vec<String> = opts
            .and_then(|o| o.get("excludedRunes"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["$props".into(), "$derived".into()]);
        let destructuring_all = opts
            .and_then(|o| o.get("destructuring"))
            .and_then(|v| v.as_str())
            == Some("all");

        let mut findings: Vec<Finding> = Vec::new();
        if let Some(s) = ctx.instance_semantic {
            collect(s, ctx.instance_content_offset, &excluded_runes, destructuring_all, &mut findings);
        }
        if let Some(s) = ctx.module_semantic {
            collect(s, ctx.module_content_offset, &excluded_runes, destructuring_all, &mut findings);
        }
        for f in findings {
            match f.fix {
                Some(fix) => ctx.diagnostic_with_fix(f.message, f.span, fix),
                None => ctx.diagnostic(f.message, f.span),
            }
        }
    }
}

fn collect<'a>(
    semantic: &Semantic<'a>,
    content_offset: u32,
    excluded_runes: &[String],
    destructuring_all: bool,
    findings: &mut Vec<Finding>,
) {
    let scoping = semantic.scoping();
    for node in semantic.nodes().iter() {
        let AstKind::VariableDeclaration(vd) = node.kind() else {
            continue;
        };
        if vd.kind != VariableDeclarationKind::Let {
            continue;
        }

        // Vendor's whole-declaration short-circuit: if any declarator's init is
        // a rune from `excludedRunes`, the entire declaration is skipped —
        // not just the affected declarator.
        let any_excluded = vd.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .and_then(rune_name)
                .is_some_and(|rune| excluded_runes.iter().any(|r| r == rune))
        });
        if any_excluded {
            continue;
        }

        // For each declarator, examine the bindings it introduces.
        let mut per_declarator: Vec<Vec<(String, Span)>> = Vec::new();
        let mut total_bindings = 0;
        let mut total_eligible = 0;
        for decl in &vd.declarations {
            // Skip declarators with no initializer — `let x;` cannot be
            // converted to `const`.
            if decl.init.is_none() {
                per_declarator.push(Vec::new());
                continue;
            }
            let mut bindings: Vec<(SymbolId, String, Span)> = Vec::new();
            collect_pattern_bindings(&decl.id, &mut bindings);
            total_bindings += bindings.len();

            let eligible: Vec<(String, Span)> = bindings
                .iter()
                .filter(|(sid, _, _)| {
                    !scoping
                        .get_resolved_references(*sid)
                        .any(|r| r.is_write())
                })
                .map(|(_, name, span)| (name.clone(), *span))
                .collect();
            total_eligible += eligible.len();

            // `destructuring: 'all'` requires every binding in the *pattern*
            // to be eligible before we report. `'any'` (default) reports
            // whatever's eligible.
            let pattern_passes = if destructuring_all && bindings.len() > 1 {
                eligible.len() == bindings.len()
            } else {
                !eligible.is_empty()
            };
            per_declarator.push(if pattern_passes {
                eligible
            } else {
                Vec::new()
            });
        }

        let any_to_report = per_declarator.iter().any(|v| !v.is_empty());
        if !any_to_report {
            continue;
        }

        // Autofix only when every binding in the whole declaration is
        // eligible (matches ESLint core's behavior — partial declarations
        // would leave the keyword inconsistent with the un-fixed members).
        let whole_declaration_eligible = total_bindings > 0 && total_eligible == total_bindings;
        let let_keyword_span = Span::new(
            content_offset + vd.span.start,
            content_offset + vd.span.start + 3,
        );

        for eligible in &per_declarator {
            for (name, binding_span) in eligible {
                let abs_span = Span::new(
                    content_offset + binding_span.start,
                    content_offset + binding_span.end,
                );
                let message = format!("'{}' is never reassigned. Use 'const' instead.", name);
                let fix = whole_declaration_eligible.then(|| Fix {
                    span: let_keyword_span,
                    replacement: "const".into(),
                });
                findings.push(Finding {
                    message,
                    span: abs_span,
                    fix,
                });
            }
        }
    }
}

/// Append every `BindingIdentifier` reachable inside `pat` to `out`, along
/// with its resolved `SymbolId` and source-span.
fn collect_pattern_bindings<'a>(
    pat: &BindingPattern<'a>,
    out: &mut Vec<(SymbolId, String, Span)>,
) {
    match pat {
        BindingPattern::BindingIdentifier(id) => {
            if let Some(sid) = id.symbol_id.get() {
                out.push((sid, id.name.to_string(), id.span));
            }
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_bindings(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_bindings(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_pattern_bindings(el, out);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_bindings(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(inner) => {
            collect_pattern_bindings(&inner.left, out);
        }
    }
}

/// For expressions like `$state(...)`, `$props()`, `$derived(...)`, or the
/// shorthand `$derived` / `$props.id` accesses, return the leading `$foo` name.
fn rune_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::CallExpression(ce) => match &ce.callee {
            Expression::Identifier(id) if id.name.starts_with('$') => Some(id.name.as_str()),
            Expression::StaticMemberExpression(mem) => {
                if let Expression::Identifier(id) = &mem.object {
                    if id.name.starts_with('$') {
                        return Some(id.name.as_str());
                    }
                }
                None
            }
            _ => None,
        },
        Expression::Identifier(id) if id.name.starts_with('$') => Some(id.name.as_str()),
        Expression::StaticMemberExpression(mem) => {
            if let Expression::Identifier(id) = &mem.object {
                if id.name.starts_with('$') {
                    return Some(id.name.as_str());
                }
            }
            None
        }
        _ => None,
    }
}
