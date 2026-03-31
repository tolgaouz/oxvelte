//! `svelte/system` — internal system rule for Svelte component validation.
//! ⭐ Recommended
//!
//! Checks for common structural issues in Svelte components such as
//! multiple `<script>` or `<style>` blocks (which is handled by the parser)
//! and other basic structural constraints.

use crate::linter::{LintContext, Rule};
use crate::ast::{TemplateNode, Fragment};

pub struct System;

impl Rule for System {
    fn name(&self) -> &'static str {
        "svelte/system"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Check for <script> or <style> elements inside the template body,
        // but skip <style> inside <svg> elements (valid SVG/HTML).
        walk_with_parent(&ctx.ast.html, None, false, ctx);
    }
}

fn walk_with_parent(fragment: &Fragment, parent_name: Option<&str>, inside_svg: bool, ctx: &mut LintContext<'_>) {
    walk_with_parent_nodes(&fragment.nodes, parent_name, inside_svg, ctx);
}

fn walk_with_parent_nodes(nodes: &[TemplateNode], parent_name: Option<&str>, inside_svg: bool, ctx: &mut LintContext<'_>) {
    for node in nodes {
        if let TemplateNode::Element(el) = node {
            if el.name == "script" && !matches!(parent_name, Some("svelte:head")) {
                ctx.diagnostic(
                    "`<script>` should be at the top level of the component, not nested inside markup.",
                    el.span,
                );
            }
            if el.name == "style" && !inside_svg && !matches!(parent_name, Some("svelte:head")) {
                ctx.diagnostic(
                    "`<style>` should be at the top level of the component, not nested inside markup.",
                    el.span,
                );
            }
            let is_svg = inside_svg || el.name == "svg";
            walk_with_parent_nodes(&el.children, Some(el.name.as_str()), is_svg, ctx);
        } else {
            walk_block_children(node, parent_name, inside_svg, ctx);
        }
    }
}

fn walk_block_children(node: &TemplateNode, parent_name: Option<&str>, inside_svg: bool, ctx: &mut LintContext<'_>) {
    match node {
        TemplateNode::IfBlock(block) => {
            walk_with_parent_nodes(&block.consequent.nodes, parent_name, inside_svg, ctx);
            if let Some(alt) = &block.alternate {
                walk_block_children(alt, parent_name, inside_svg, ctx);
            }
        }
        TemplateNode::EachBlock(block) => {
            walk_with_parent_nodes(&block.body.nodes, parent_name, inside_svg, ctx);
            if let Some(fb) = &block.fallback {
                walk_with_parent_nodes(&fb.nodes, parent_name, inside_svg, ctx);
            }
        }
        TemplateNode::AwaitBlock(block) => {
            if let Some(p) = &block.pending { walk_with_parent_nodes(&p.nodes, parent_name, inside_svg, ctx); }
            if let Some(t) = &block.then { walk_with_parent_nodes(&t.nodes, parent_name, inside_svg, ctx); }
            if let Some(c) = &block.catch { walk_with_parent_nodes(&c.nodes, parent_name, inside_svg, ctx); }
        }
        TemplateNode::KeyBlock(block) => walk_with_parent_nodes(&block.body.nodes, parent_name, inside_svg, ctx),
        TemplateNode::SnippetBlock(block) => walk_with_parent_nodes(&block.body.nodes, parent_name, inside_svg, ctx),
        _ => {}
    }
}
