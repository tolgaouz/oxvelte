//! `svelte/no-unused-svelte-ignore` — disallow unused svelte-ignore comments.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct NoUnusedSvelteIgnore;

impl Rule for NoUnusedSvelteIgnore {
    fn name(&self) -> &'static str {
        "svelte/no-unused-svelte-ignore"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Comment(comment) = node {
                if comment.data.trim_start().starts_with("svelte-ignore") {
                    let after = comment.data.trim_start().strip_prefix("svelte-ignore").unwrap_or("");
                    if after.trim().is_empty() {
                        ctx.diagnostic(
                            "svelte-ignore comment must include the code",
                            comment.span,
                        );
                    }
                }
            }
        });

        // Also check JS comments in script blocks
        for script in [&ctx.ast.instance, &ctx.ast.module].iter().filter_map(|s| s.as_ref()) {
            let content = &script.content;
            let base = script.span.start as usize;
            let source = ctx.source;
            let tag_text = &source[base..script.span.end as usize];
            let content_offset = tag_text.find('>').map(|p| base + p + 1).unwrap_or(base);

            for (pos, _) in content.match_indices("// svelte-ignore") {
                let after = &content[pos + 16..];
                let line_end = after.find('\n').unwrap_or(after.len());
                let codes = after[..line_end].trim();
                if codes.is_empty() {
                    let src_pos = content_offset + pos;
                    ctx.diagnostic(
                        "svelte-ignore comment must include the code",
                        oxc::span::Span::new(src_pos as u32, (src_pos + 16) as u32),
                    );
                }
            }
        }
    }
}
