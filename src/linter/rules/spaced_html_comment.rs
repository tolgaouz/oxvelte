//! `svelte/spaced-html-comment` — enforce consistent spacing after `<!--` and before `-->`.
//! 🔧 Fixable

use crate::ast::TemplateNode;
use crate::linter::{walk_template_nodes, Fix, LintContext, Rule};
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
        let mode_never = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            == Some("never");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Comment(comment) = node else {
                return;
            };
            let data = &comment.data;
            if data.trim().is_empty() {
                return;
            }
            let s = comment.span;

            if mode_never {
                let leading_len = data
                    .chars()
                    .take_while(|ch| matches!(ch, ' ' | '\t'))
                    .map(char::len_utf8)
                    .sum::<usize>();
                if leading_len > 0 {
                    ctx.diagnostic_with_fix(
                        "Unexpected space or tab after '<!--' in comment.",
                        s,
                        Fix {
                            span: Span::new(s.start + 4, s.start + 4 + leading_len as u32),
                            replacement: String::new(),
                        },
                    );
                }
                let trailing_len = data
                    .chars()
                    .rev()
                    .take_while(|ch| matches!(ch, ' ' | '\t'))
                    .map(char::len_utf8)
                    .sum::<usize>();
                let trailing_start = data.len().saturating_sub(trailing_len);
                let trailing_is_after_content = trailing_len > 0
                    && data[..trailing_start]
                        .chars()
                        .last()
                        .is_some_and(|ch| !ch.is_whitespace());
                if trailing_is_after_content {
                    ctx.diagnostic_with_fix(
                        "Unexpected space or tab before '-->' in comment.",
                        s,
                        Fix {
                            span: Span::new(s.end - 3 - trailing_len as u32, s.end - 3),
                            replacement: String::new(),
                        },
                    );
                }
            } else {
                if !data.chars().next().unwrap_or(' ').is_whitespace() {
                    ctx.diagnostic_with_fix(
                        "Expected space or tab after '<!--' in comment.",
                        s,
                        Fix {
                            span: Span::new(s.start + 4, s.start + 4),
                            replacement: " ".to_string(),
                        },
                    );
                }
                if !data.chars().last().unwrap_or(' ').is_whitespace() {
                    ctx.diagnostic_with_fix(
                        "Expected space or tab before '-->' in comment.",
                        s,
                        Fix {
                            span: Span::new(s.end - 3, s.end - 3),
                            replacement: " ".to_string(),
                        },
                    );
                }
            }
        });
    }
}
