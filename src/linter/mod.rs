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

/// Context provided to lint rules during execution.
pub struct LintContext<'a> {
    pub ast: &'a SvelteAst,
    pub source: &'a str,
    diagnostics: Vec<LintDiagnostic>,
    current_rule: &'static str,
}

impl<'a> LintContext<'a> {
    pub fn new(ast: &'a SvelteAst, source: &'a str) -> Self {
        Self { ast, source, diagnostics: Vec::new(), current_rule: "" }
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

    pub fn lint(&self, ast: &SvelteAst, source: &str) -> Vec<LintDiagnostic> {
        let mut ctx = LintContext::new(ast, source);
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
    for node in &fragment.nodes {
        visitor(node);
        match node {
            TemplateNode::Element(el) => {
                let child_frag = Fragment { nodes: el.children.clone(), span: el.span };
                walk_template_nodes(&child_frag, visitor);
            }
            TemplateNode::IfBlock(block) => {
                walk_template_nodes(&block.consequent, visitor);
                if let Some(alt) = &block.alternate {
                    // Walk the alternate as if it were in a fragment so visitors see it
                    let wrapper = Fragment {
                        nodes: vec![*alt.clone()],
                        span: block.span,
                    };
                    walk_template_nodes(&wrapper, visitor);
                }
            }
            TemplateNode::EachBlock(block) => {
                walk_template_nodes(&block.body, visitor);
                if let Some(fb) = &block.fallback { walk_template_nodes(fb, visitor); }
            }
            TemplateNode::AwaitBlock(block) => {
                if let Some(p) = &block.pending { walk_template_nodes(p, visitor); }
                if let Some(t) = &block.then { walk_template_nodes(t, visitor); }
                if let Some(c) = &block.catch { walk_template_nodes(c, visitor); }
            }
            TemplateNode::KeyBlock(block) => walk_template_nodes(&block.body, visitor),
            TemplateNode::SnippetBlock(block) => walk_template_nodes(&block.body, visitor),
            _ => {}
        }
    }
}
