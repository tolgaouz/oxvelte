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
            if let TemplateNode::Comment(c) = node {
                if let Some(after) = c.data.trim_start().strip_prefix("svelte-ignore") {
                    if after.trim().is_empty() { ctx.diagnostic("svelte-ignore comment must include the code", c.span); }
                }
            }
        });
        for script in [&ctx.ast.instance, &ctx.ast.module].iter().filter_map(|s| s.as_ref()) {
            let base = script.span.start as usize;
            let co = ctx.source[base..].find('>').map(|p| base + p + 1).unwrap_or(base);
            for (pos, _) in script.content.match_indices("// svelte-ignore") {
                let after = &script.content[pos + 16..];
                if after[..after.find('\n').unwrap_or(after.len())].trim().is_empty() {
                    let sp = co + pos;
                    ctx.diagnostic("svelte-ignore comment must include the code", oxc::span::Span::new(sp as u32, (sp + 16) as u32));
                }
            }
        }
    }
}
