//! `svelte/html-closing-bracket-new-line` — require or disallow a newline before
//! the closing bracket of elements.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct HtmlClosingBracketNewLine;

impl Rule for HtmlClosingBracketNewLine {
    fn name(&self) -> &'static str {
        "svelte/html-closing-bracket-new-line"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.attributes.is_empty() { return; }
                let tag_text = &ctx.source[el.span.start as usize..el.span.end as usize];
                // Find the closing bracket (> or />)
                // For multiline elements, the closing bracket should be on the same line as the last attribute
                // or on its own line (not separated by empty lines)
                let open_end = tag_text.find('>').unwrap_or(tag_text.len());
                let before_bracket = &tag_text[..open_end];

                // Check if there's an empty line between the last non-whitespace content and the bracket
                let lines: Vec<&str> = before_bracket.lines().collect();
                if lines.len() > 1 {
                    // Check for empty lines before the closing bracket
                    let mut found_empty = false;
                    let mut i = lines.len() - 1;
                    while i > 0 {
                        let line = lines[i].trim();
                        if line.is_empty() {
                            found_empty = true;
                        } else if line == "/" || line == "/>" {
                            // This is the closing bracket line, check if there were empty lines before it
                            if found_empty {
                                let bracket_start = el.span.start + before_bracket.rfind(line).unwrap_or(0) as u32;
                                ctx.diagnostic(
                                    "Unexpected empty line before closing bracket.",
                                    oxc::span::Span::new(bracket_start, bracket_start + line.len() as u32),
                                );
                            }
                            break;
                        } else {
                            break;
                        }
                        if i == 0 { break; }
                        i -= 1;
                    }
                }
            }
        });
    }
}
