//! `svelte/system` — internal system rule for Svelte component validation.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use crate::ast::TemplateNode;

pub struct System;

impl Rule for System {
    fn name(&self) -> &'static str { "svelte/system" }
    fn is_recommended(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk(&ctx.ast.html.nodes, None, false, ctx);
    }
}

fn walk(nodes: &[TemplateNode], parent: Option<&str>, in_svg: bool, ctx: &mut LintContext<'_>) {
    for node in nodes {
        if let TemplateNode::Element(el) = node {
            let in_head = matches!(parent, Some("svelte:head"));
            if el.name == "script" && !in_head {
                ctx.diagnostic("`<script>` should be at the top level of the component, not nested inside markup.", el.span);
            }
            if el.name == "style" && !in_svg && !in_head {
                ctx.diagnostic("`<style>` should be at the top level of the component, not nested inside markup.", el.span);
            }
            walk(&el.children, Some(el.name.as_str()), in_svg || el.name == "svg", ctx);
        } else {
            walk_block(node, parent, in_svg, ctx);
        }
    }
}

fn walk_block(node: &TemplateNode, parent: Option<&str>, in_svg: bool, ctx: &mut LintContext<'_>) {
    match node {
        TemplateNode::IfBlock(b) => {
            walk(&b.consequent.nodes, parent, in_svg, ctx);
            if let Some(alt) = &b.alternate { walk_block(alt, parent, in_svg, ctx); }
        }
        TemplateNode::EachBlock(b) => {
            walk(&b.body.nodes, parent, in_svg, ctx);
            if let Some(fb) = &b.fallback { walk(&fb.nodes, parent, in_svg, ctx); }
        }
        TemplateNode::AwaitBlock(b) => {
            for f in [&b.pending, &b.then, &b.catch].into_iter().flatten() { walk(&f.nodes, parent, in_svg, ctx); }
        }
        TemplateNode::KeyBlock(b) => walk(&b.body.nodes, parent, in_svg, ctx),
        TemplateNode::SnippetBlock(b) => walk(&b.body.nodes, parent, in_svg, ctx),
        _ => {}
    }
}
