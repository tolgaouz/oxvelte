//! `svelte/spaced-html-comment` — enforce consistent spacing after `<!--` and before `-->`.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule, Fix};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct SpacedHtmlComment;

impl Rule for SpacedHtmlComment {
    fn name(&self) -> &'static str {
        "svelte/spaced-html-comment"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let mode_never = ctx.config.options.as_ref()
            .and_then(|v| v.as_array()).and_then(|arr| arr.first())
            .and_then(|v| v.as_str()) == Some("never");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Comment(comment) = node else { return };
            let data = &comment.data;
            if data.trim().is_empty() { return; }
            let s = comment.span;

            if mode_never {
                if data.starts_with(' ') || data.starts_with('\t') {
                    ctx.diagnostic_with_fix("Unexpected space or tab after '<!--' in comment.", s,
                        Fix { span: Span::new(s.start + 4, s.start + 5), replacement: String::new() });
                }
                let trailing = (data.ends_with(' ') || data.ends_with('\t'))
                    && !data.rfind('\n').map(|p| data[p+1..].chars().all(|c| c == ' ' || c == '\t')).unwrap_or(false);
                if trailing {
                    ctx.diagnostic_with_fix("Unexpected space or tab before '-->' in comment.", s,
                        Fix { span: Span::new(s.end - 4, s.end - 3), replacement: String::new() });
                }
            } else {
                if !data.chars().next().unwrap_or(' ').is_whitespace() {
                    ctx.diagnostic_with_fix("Expected space or tab after '<!--' in comment.", s,
                        Fix { span: Span::new(s.start + 4, s.start + 4), replacement: " ".to_string() });
                }
                if !data.chars().last().unwrap_or(' ').is_whitespace() {
                    ctx.diagnostic_with_fix("Expected space or tab before '-->' in comment.", s,
                        Fix { span: Span::new(s.end - 3, s.end - 3), replacement: " ".to_string() });
                }
            }
        });
    }
}
