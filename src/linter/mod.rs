//! Svelte linter. Runs lint rules on a parsed `SvelteAst`.

pub mod rules;

use oxc::span::Span;
use crate::ast::*;

/// A lint diagnostic.
#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    pub rule_name: &'static str,
    pub message: String,
    pub span: Span,
    pub fix: Option<Fix>,
}

/// An auto-fix: replace a span of source text.
#[derive(Debug, Clone)]
pub struct Fix {
    pub span: Span,
    pub replacement: String,
}

/// Rule configuration loaded from config files.
#[derive(Debug, Clone, Default)]
pub struct RuleConfig {
    /// Raw JSON options from the config file
    pub options: Option<serde_json::Value>,
    /// Parsed settings
    pub settings: Option<serde_json::Value>,
}

/// Context provided to lint rules during execution.
pub struct LintContext<'a> {
    pub ast: &'a SvelteAst,
    pub source: &'a str,
    pub config: RuleConfig,
    /// Path to the file being linted (for cross-file resolution)
    pub file_path: Option<String>,
    diagnostics: Vec<LintDiagnostic>,
    current_rule: &'static str,
}

impl<'a> LintContext<'a> {
    pub fn new(ast: &'a SvelteAst, source: &'a str) -> Self {
        Self { ast, source, config: RuleConfig::default(), file_path: None, diagnostics: Vec::new(), current_rule: "" }
    }

    pub fn with_config(ast: &'a SvelteAst, source: &'a str, config: RuleConfig) -> Self {
        Self { ast, source, config, file_path: None, diagnostics: Vec::new(), current_rule: "" }
    }

    pub fn diagnostic(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(LintDiagnostic {
            rule_name: self.current_rule, message: message.into(), span, fix: None,
        });
    }

    pub fn diagnostic_with_fix(&mut self, message: impl Into<String>, span: Span, fix: Fix) {
        self.diagnostics.push(LintDiagnostic {
            rule_name: self.current_rule, message: message.into(), span, fix: Some(fix),
        });
    }

    fn set_rule(&mut self, name: &'static str) { self.current_rule = name; }
    pub fn into_diagnostics(self) -> Vec<LintDiagnostic> { self.diagnostics }
}

/// The trait all lint rules implement.
pub trait Rule: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_recommended(&self) -> bool { false }
    fn is_fixable(&self) -> bool { false }
    fn run(&self, ctx: &mut LintContext);
}

/// The linter: holds rules and runs them on parsed files.
pub struct Linter {
    rules: Vec<Box<dyn Rule>>,
}

impl Linter {
    pub fn recommended() -> Self {
        Self { rules: rules::recommended_rules() }
    }
    pub fn all() -> Self {
        Self { rules: rules::all_rules() }
    }

    pub fn rules(&self) -> &[Box<dyn Rule>] {
        &self.rules
    }

    pub fn lint(&self, ast: &SvelteAst, source: &str) -> Vec<LintDiagnostic> {
        let mut ctx = LintContext::new(ast, source);
        for rule in &self.rules {
            ctx.set_rule(rule.name());
            rule.run(&mut ctx);
        }
        ctx.into_diagnostics()
    }

    pub fn lint_with_config(&self, ast: &SvelteAst, source: &str, config: RuleConfig) -> Vec<LintDiagnostic> {
        let mut ctx = LintContext::with_config(ast, source, config);
        for rule in &self.rules {
            ctx.set_rule(rule.name());
            rule.run(&mut ctx);
        }
        ctx.into_diagnostics()
    }

    pub fn lint_with_config_and_path(&self, ast: &SvelteAst, source: &str, config: RuleConfig, file_path: &str) -> Vec<LintDiagnostic> {
        let mut ctx = LintContext::with_config(ast, source, config);
        ctx.file_path = Some(file_path.to_string());
        for rule in &self.rules {
            ctx.set_rule(rule.name());
            rule.run(&mut ctx);
        }
        ctx.into_diagnostics()
    }
}

/// Walk all template nodes recursively, calling visitor on each.
pub fn walk_template_nodes<F>(fragment: &Fragment, visitor: &mut F)
where F: FnMut(&TemplateNode)
{
    walk_nodes(&fragment.nodes, visitor);
}

fn walk_nodes<F>(nodes: &[TemplateNode], visitor: &mut F)
where F: FnMut(&TemplateNode)
{
    for node in nodes {
        visitor(node);
        match node {
            TemplateNode::Element(el) => {
                walk_nodes(&el.children, visitor);
            }
            TemplateNode::IfBlock(block) => {
                walk_nodes(&block.consequent.nodes, visitor);
                if let Some(alt) = &block.alternate {
                    visitor(alt);
                    // Recurse into the alternate node
                    match alt.as_ref() {
                        TemplateNode::IfBlock(ib) => {
                            walk_nodes(&ib.consequent.nodes, visitor);
                            if let Some(a) = &ib.alternate { walk_alt(a, visitor); }
                        }
                        _ => {}
                    }
                }
            }
            TemplateNode::EachBlock(block) => {
                walk_nodes(&block.body.nodes, visitor);
                if let Some(fb) = &block.fallback { walk_nodes(&fb.nodes, visitor); }
            }
            TemplateNode::AwaitBlock(block) => {
                if let Some(p) = &block.pending { walk_nodes(&p.nodes, visitor); }
                if let Some(t) = &block.then { walk_nodes(&t.nodes, visitor); }
                if let Some(c) = &block.catch { walk_nodes(&c.nodes, visitor); }
            }
            TemplateNode::KeyBlock(block) => walk_nodes(&block.body.nodes, visitor),
            TemplateNode::SnippetBlock(block) => walk_nodes(&block.body.nodes, visitor),
            _ => {}
        }
    }
}

fn walk_alt<F>(alt: &Box<TemplateNode>, visitor: &mut F)
where F: FnMut(&TemplateNode)
{
    visitor(alt);
    if let TemplateNode::IfBlock(ib) = alt.as_ref() {
        walk_nodes(&ib.consequent.nodes, visitor);
        if let Some(a) = &ib.alternate { walk_alt(a, visitor); }
    }
}

/// Parse import statements from script content.
/// Returns a list of (local_name, imported_name, source_module) tuples.
pub fn parse_imports(content: &str) -> Vec<(String, String, String)> {
    let mut imports = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = content[search_from..].find("import ") {
        let abs = search_from + pos;
        // Make sure it's at a statement boundary
        if abs > 0 {
            let prev = content.as_bytes()[abs - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search_from = abs + 7;
                continue;
            }
        }
        let rest = &content[abs + 7..];
        // Find the "from" keyword and module string
        if let Some(from_pos) = rest.find("from ") {
            let specifier_text = &rest[..from_pos].trim();
            let module_text = &rest[from_pos + 5..];
            let module = extract_string_literal(module_text.trim());
            if let Some(module) = module {
                // Parse specifiers
                if specifier_text.starts_with('{') {
                    // Named imports: import { a, b as c } from 'mod'
                    let inner = specifier_text.trim_start_matches('{').trim_end_matches('}');
                    for spec in inner.split(',') {
                        let spec = spec.trim();
                        if spec.is_empty() { continue; }
                        if let Some(as_pos) = spec.find(" as ") {
                            let imported = spec[..as_pos].trim();
                            let local = spec[as_pos + 4..].trim();
                            imports.push((local.to_string(), imported.to_string(), module.clone()));
                        } else {
                            imports.push((spec.to_string(), spec.to_string(), module.clone()));
                        }
                    }
                } else if specifier_text.starts_with("* as ") {
                    // Namespace import: import * as name from 'mod'
                    let local = specifier_text[5..].trim();
                    imports.push((local.to_string(), "*".to_string(), module.clone()));
                } else {
                    // Default import: import name from 'mod'
                    let local = specifier_text.trim();
                    if !local.is_empty() {
                        imports.push((local.to_string(), "default".to_string(), module.clone()));
                    }
                }
            }
        }
        search_from = abs + 7;
    }
    imports
}

fn extract_string_literal(s: &str) -> Option<String> {
    if s.len() < 2 { return None; }
    let quote = s.as_bytes()[0];
    if quote != b'\'' && quote != b'"' && quote != b'`' { return None; }
    if let Some(end) = s[1..].find(quote as char) {
        return Some(s[1..1 + end].to_string());
    }
    None
}
