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
        // Config: "always" | "never", default "always"
        let mode_never = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(|s| s == "never")
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Comment(comment) = node {
                let data = &comment.data;
                if data.trim().is_empty() {
                    return;
                }

                if mode_never {
                    // "never" mode: no spaces should be directly after <!-- or before -->
                    if data.starts_with(' ') || data.starts_with('\t') {
                        ctx.diagnostic_with_fix(
                            "Unexpected space or tab after '<!--' in comment.",
                            comment.span,
                            Fix { span: Span::new(comment.span.start + 4, comment.span.start + 5), replacement: String::new() },
                        );
                    }
                    // For closing, only flag if the space before --> is on the same line
                    // (not just indentation from a newline)
                    let has_trailing_space = (data.ends_with(' ') || data.ends_with('\t'))
                        && !data.rfind('\n').map(|nl_pos| data[nl_pos+1..].chars().all(|c| c == ' ' || c == '\t')).unwrap_or(false);
                    if has_trailing_space {
                        ctx.diagnostic_with_fix(
                            "Unexpected space or tab before '-->' in comment.",
                            comment.span,
                            Fix { span: Span::new(comment.span.end - 4, comment.span.end - 3), replacement: String::new() },
                        );
                    }
                } else {
                    // "always" mode (default): spaces/whitespace required after <!-- and before -->
                    let first_char = data.chars().next().unwrap_or(' ');
                    if !first_char.is_whitespace() {
                        let fix_span = Span::new(comment.span.start + 4, comment.span.start + 4);
                        ctx.diagnostic_with_fix(
                            "Expected space or tab after '<!--' in comment.",
                            comment.span,
                            Fix { span: fix_span, replacement: " ".to_string() },
                        );
                    }
                    let last_char = data.chars().last().unwrap_or(' ');
                    if !last_char.is_whitespace() {
                        let fix_span = Span::new(comment.span.end - 3, comment.span.end - 3);
                        ctx.diagnostic_with_fix(
                            "Expected space or tab before '-->' in comment.",
                            comment.span,
                            Fix { span: fix_span, replacement: " ".to_string() },
                        );
                    }
                }
            }
        });
    }
}
