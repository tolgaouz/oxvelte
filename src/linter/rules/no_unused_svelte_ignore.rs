//! `svelte/no-unused-svelte-ignore` — disallow unused svelte-ignore comments.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;
use oxc::span::Span;

pub struct NoUnusedSvelteIgnore;

impl Rule for NoUnusedSvelteIgnore {
    fn name(&self) -> &'static str {
        "svelte/no-unused-svelte-ignore"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Template HTML comments: `<!-- svelte-ignore ... -->`
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Comment(c) = node {
                if let Some(after) = c.data.trim_start().strip_prefix("svelte-ignore") {
                    if after.trim().is_empty() {
                        ctx.diagnostic("svelte-ignore comment must include the code", c.span);
                    }
                }
            }
        });

        // JS `// svelte-ignore` line comments via the parsed Program's `comments`.
        for (sem, offset) in [
            (ctx.instance_semantic, ctx.instance_content_offset),
            (ctx.module_semantic, ctx.module_content_offset),
        ]
        .into_iter()
        .filter_map(|(s, o)| s.map(|s| (s, o)))
        {
            for c in sem.nodes().program().comments.iter() {
                let text = &sem.source_text()[c.span.start as usize..c.span.end as usize];
                let body = if c.is_line() {
                    text.strip_prefix("//").unwrap_or(text)
                } else {
                    text.strip_prefix("/*")
                        .and_then(|t| t.strip_suffix("*/"))
                        .unwrap_or(text)
                };
                let trimmed = body.trim_start();
                if let Some(rest) = trimmed.strip_prefix("svelte-ignore") {
                    if rest.trim().is_empty() {
                        let s = offset + c.span.start;
                        let e = offset + c.span.end;
                        ctx.diagnostic(
                            "svelte-ignore comment must include the code",
                            Span::new(s, e),
                        );
                    }
                }
            }
        }
    }
}
