//! `svelte/mustache-spacing` — enforce consistent spacing inside mustache braces `{ }`.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct MustacheSpacing;

impl Rule for MustacheSpacing {
    fn name(&self) -> &'static str {
        "svelte/mustache-spacing"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::MustacheTag(tag) = node {
                let start = tag.span.start as usize;
                let end = tag.span.end as usize;
                if end <= ctx.source.len() && end > start + 2 {
                    let src = &ctx.source[start..end];
                    // Expect `{ expr }` with spaces after `{` and before `}`.
                    if src.starts_with('{') && src.ends_with('}') {
                        let inner = &src[1..src.len() - 1];
                        if !inner.starts_with(' ') || !inner.ends_with(' ') {
                            ctx.diagnostic(
                                "Expected spaces inside mustache braces: `{ expr }`.",
                                Span::new(tag.span.start, tag.span.end),
                            );
                        }
                    }
                }
            }
        });
    }
}
