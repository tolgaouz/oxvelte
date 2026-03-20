//! `svelte/max-attributes-per-line` — enforce the maximum number of attributes per line.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

/// Default maximum attributes allowed on a single line.
const DEFAULT_MAX: usize = 1;

pub struct MaxAttributesPerLine;

impl Rule for MaxAttributesPerLine {
    fn name(&self) -> &'static str {
        "svelte/max-attributes-per-line"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.attributes.len() > DEFAULT_MAX {
                    // Check if all attributes are on the same line by inspecting the span.
                    let start = el.span.start as usize;
                    let end = el.span.end as usize;
                    if end <= ctx.source.len() {
                        let tag_src = &ctx.source[start..end];
                        // Find the opening tag portion (up to the first `>`).
                        if let Some(close_pos) = tag_src.find('>') {
                            let opening = &tag_src[..close_pos];
                            // If the opening tag is all on one line, flag it.
                            if !opening.contains('\n') {
                                ctx.diagnostic(
                                    format!(
                                        "Element has {} attributes on one line (max {}).",
                                        el.attributes.len(),
                                        DEFAULT_MAX
                                    ),
                                    el.span,
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}
