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
        // Parse excludedRunes from config. Vendor default: ['$props', '$derived']
        let excluded_runes: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|o| o.get("excludedRunes"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["$props".into(), "$derived".into()]);

        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            if content.trim().is_empty() {
                return;
            }

            // Compute the byte offset where script content starts in the full source.
            let tag_text = &ctx.source[script.span.start as usize..script.span.end as usize];
            let gt = tag_text.find('>').unwrap_or(0);
            let content_offset = script.span.start as usize + gt + 1;

            // Determine source type based on lang attribute
            let is_ts = script.lang.as_deref() == Some("ts")
                || script.lang.as_deref() == Some("typescript");

            self.check_script(ctx, content, content_offset, is_ts, &excluded_runes);
        }
    }
}

impl PreferConst {
    fn check_script(
        &self,
        ctx: &mut LintContext,
        content: &str,
        content_offset: usize,
        is_ts: bool,
        excluded_runes: &[String],
    ) {
        use oxc::allocator::Allocator;
        use oxc::ast::AstKind;
        use oxc::parser::Parser;
        use oxc::semantic::SemanticBuilder;
        use oxc::span::SourceType;

        let alloc = Allocator::default();
        let source_type = if is_ts {
            SourceType::ts()
        } else {
            SourceType::mjs()
        };

        let parse_result = Parser::new(&alloc, content, source_type).parse();
        if !parse_result.errors.is_empty() {
            return; // Don't lint files with parse errors
        }

        let semantic_ret = SemanticBuilder::new().build(&parse_result.program);
        let semantic = semantic_ret.semantic;

        let scoping = semantic.scoping();
        let nodes = semantic.nodes();

        // Iterate over all symbols
        for symbol_id in scoping.symbol_ids() {
            let symbol_name = scoping.symbol_name(symbol_id);
            let flags = scoping.symbol_flags(symbol_id);

            // We only care about `let` declarations:
            // BlockScopedVariable but NOT ConstVariable
            if !flags.intersects(oxc::semantic::SymbolFlags::BlockScopedVariable) {
                continue;
            }
            if flags.intersects(oxc::semantic::SymbolFlags::ConstVariable) {
                continue;
            }

            // Check if there are any write references (reassignment).
            // The declaration itself is NOT counted as a reference by oxc semantic -
            // references are only usages after the declaration.
            let has_write = scoping
                .get_resolved_references(symbol_id)
                .any(|r| r.is_write());

            if has_write {
                continue; // Variable is reassigned, skip
            }

            // Walk up from the declaration node to find the VariableDeclarator,
            // then check if its initializer is an excluded rune.
            let decl_node_id = scoping.symbol_declaration(symbol_id);
            let mut skip = false;

            // Check the declaration node itself and its ancestors for VariableDeclarator
            let node_ids = std::iter::once(decl_node_id).chain(nodes.ancestor_ids(decl_node_id));
            for node_id in node_ids {
                let kind = nodes.kind(node_id);
                if let AstKind::VariableDeclarator(declarator) = kind {
                    if let Some(init) = &declarator.init {
                        let init_start = init.span().start as usize;
                        let init_end = init.span().end as usize;
                        if init_end <= content.len() {
                            let init_text = &content[init_start..init_end];
                            if let Some(rune) = extract_rune_name(init_text) {
                                if excluded_runes.iter().any(|r| r == rune) {
                                    skip = true;
                                }
                            }
                        }
                    }
                    break;
                }
            }

            if skip {
                continue;
            }

            // Report diagnostic with proper offset into the full .svelte source
            let symbol_span = scoping.symbol_span(symbol_id);
            let abs_start = content_offset + symbol_span.start as usize;
            let abs_end = content_offset + symbol_span.end as usize;

            ctx.diagnostic(
                format!("'{}' is never reassigned. Use 'const' instead.", symbol_name),
                Span::new(abs_start as u32, abs_end as u32),
            );
        }
    }
}
