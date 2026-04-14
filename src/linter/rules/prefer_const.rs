//! `svelte/prefer-const` — require `const` declarations for variables that are never reassigned.
//! 🔧 Fixable
//!
//! Uses the shared `oxc_semantic` model exposed on `LintContext`. Iterates
//! block-scoped symbols (`let` bindings), flags the ones with no write
//! references. Declarations initialized with an excluded rune (`$props`,
//! `$derived` by default) are kept skipped.

use crate::linter::{LintContext, Rule};
use oxc::ast::AstKind;
use oxc::ast::ast::Expression;
use oxc::span::Span;

pub struct PreferConst;

impl Rule for PreferConst {
    fn name(&self) -> &'static str {
        "svelte/prefer-const"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let excluded_runes: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|o| o.get("excludedRunes"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["$props".into(), "$derived".into()]);

        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;
        let scoping = semantic.scoping();
        let nodes = semantic.nodes();

        for symbol_id in scoping.symbol_ids() {
            let flags = scoping.symbol_flags(symbol_id);
            if !flags.intersects(oxc::semantic::SymbolFlags::BlockScopedVariable)
                || flags.intersects(oxc::semantic::SymbolFlags::ConstVariable)
            {
                continue;
            }
            if scoping.get_resolved_references(symbol_id).any(|r| r.is_write()) {
                continue;
            }

            let decl_node_id = scoping.symbol_declaration(symbol_id);
            let is_excluded = std::iter::once(decl_node_id)
                .chain(nodes.ancestor_ids(decl_node_id))
                .find_map(|nid| match nodes.kind(nid) {
                    AstKind::VariableDeclarator(d) => Some(d),
                    _ => None,
                })
                .and_then(|d| d.init.as_ref())
                .and_then(|init| rune_name(init))
                .map_or(false, |rune| excluded_runes.iter().any(|r| r == rune));
            if is_excluded {
                continue;
            }

            let symbol_span = scoping.symbol_span(symbol_id);
            let abs_start = content_offset + symbol_span.start;
            let abs_end = content_offset + symbol_span.end;
            ctx.diagnostic(
                format!("'{}' is never reassigned. Use 'const' instead.", scoping.symbol_name(symbol_id)),
                Span::new(abs_start, abs_end),
            );
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
