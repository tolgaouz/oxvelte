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
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Comment(comment) = node {
                let data = &comment.data;
                if !data.starts_with(' ') && !data.is_empty() {
                    let fix_span = Span::new(comment.span.start + 4, comment.span.start + 4);
                    ctx.diagnostic_with_fix(
                        "Expected a space after `<!--`.",
                        comment.span,
                        Fix { span: fix_span, replacement: " ".to_string() },
                    );
                }
                if !data.ends_with(' ') && !data.is_empty() {
                    let fix_span = Span::new(comment.span.end - 3, comment.span.end - 3);
                    ctx.diagnostic_with_fix(
                        "Expected a space before `-->`.",
                        comment.span,
                        Fix { span: fix_span, replacement: " ".to_string() },
                    );
                }
            }
        });
    }
}
