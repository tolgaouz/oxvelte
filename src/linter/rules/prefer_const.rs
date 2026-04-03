//! `svelte/prefer-const` — require `const` declarations for variables that are never reassigned.
//! 🔧 Fixable
//!
//! Uses `oxc_semantic` for proper scope-based analysis. Parses each script block
//! with `oxc::parser::Parser`, builds semantic information with `SemanticBuilder`,
//! then iterates symbols to find `let` declarations that are never reassigned.
//! Declarations initialized with excluded runes (`$props`, `$derived` by default)
//! are skipped.

use crate::linter::{LintContext, Rule};
use oxc::span::{GetSpan, Span};

pub struct PreferConst;

/// Given an initializer expression source text, extract the rune name
/// if the expression is a rune call like `$state(0)` or `$derived.by(calc())`.
///
/// - `$state(0)` → Some("$state")
/// - `$derived.by(fn)` → Some("$derived")
/// - `$props()` → Some("$props")
/// - `calc()` → None
/// - `0` → None
fn extract_rune_name(init: &str) -> Option<&str> {
    let init = init.trim();
    if !init.starts_with('$') {
        return None;
    }
    // Rune name ends at `(` (direct call) or `.` (member like $derived.by)
    let end = init.find(|c: char| c == '(' || c == '.').unwrap_or(init.len());
    let name = &init[..end];
    if name.len() > 1 && name[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(name)
    } else {
        None
    }
}

impl Rule for PreferConst {
    fn name(&self) -> &'static str {
        "svelte/prefer-const"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        use oxc::allocator::Allocator;
        use oxc::ast::AstKind;
        use oxc::parser::Parser;
        use oxc::semantic::SemanticBuilder;
        use oxc::span::SourceType;

        let excluded_runes: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|o| o.get("excludedRunes"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["$props".into(), "$derived".into()]);

        let script = match &ctx.ast.instance {
            Some(s) if !s.content.trim().is_empty() => s,
            _ => return,
        };
        let content = &script.content;
        let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
        let content_offset = script.span.start as usize + tag_text.find('>').unwrap_or(0) + 1;
        let is_ts = matches!(script.lang.as_deref(), Some("ts" | "typescript"));

        let alloc = Allocator::default();
        let source_type = if is_ts { SourceType::ts() } else { SourceType::mjs() };
        let parse_result = Parser::new(&alloc, content, source_type).parse();
        if !parse_result.errors.is_empty() { return; }

        let semantic = SemanticBuilder::new().build(&parse_result.program).semantic;
        let scoping = semantic.scoping();
        let nodes = semantic.nodes();

        for symbol_id in scoping.symbol_ids() {
            let flags = scoping.symbol_flags(symbol_id);
            if !flags.intersects(oxc::semantic::SymbolFlags::BlockScopedVariable)
                || flags.intersects(oxc::semantic::SymbolFlags::ConstVariable) {
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
                .and_then(|init| content.get(init.span().start as usize..init.span().end as usize))
                .and_then(extract_rune_name)
                .map_or(false, |rune| excluded_runes.iter().any(|r| r == rune));
            if is_excluded { continue; }

            let symbol_span = scoping.symbol_span(symbol_id);
            let abs_start = content_offset + symbol_span.start as usize;
            let abs_end = content_offset + symbol_span.end as usize;
            ctx.diagnostic(
                format!("'{}' is never reassigned. Use 'const' instead.", scoping.symbol_name(symbol_id)),
                Span::new(abs_start as u32, abs_end as u32),
            );
        }
    }
}
