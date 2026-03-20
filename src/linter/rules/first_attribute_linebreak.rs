//! `svelte/first-attribute-linebreak` — enforce the location of first attribute.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct FirstAttributeLinebreak;

impl Rule for FirstAttributeLinebreak {
    fn name(&self) -> &'static str {
        "svelte/first-attribute-linebreak"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // When an element has multiple attributes, the first attribute should be on
        // a new line. This is a simplified check using source text.
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.attributes.len() > 1 {
                    // Extract the source of the opening tag.
                    let tag_start = el.span.start as usize;
                    let src = ctx.source;
                    // Find the tag name end.
                    let tag_src = &src[tag_start..];
                    if let Some(name_end) = tag_src.find(|c: char| c.is_whitespace()) {
                        let after_name = &tag_src[name_end..];
                        // Check if first attr is on the same line as the tag.
                        let first_attr_char = after_name.trim_start();
                        if !first_attr_char.is_empty()
                            && !after_name.starts_with('\n')
                            && !after_name.starts_with("\r\n")
                        {
                            ctx.diagnostic(
                                "First attribute should be on a new line when there are multiple attributes.",
                                el.span,
                            );
                        }
                    }
                }
            }
        });
    }
}
