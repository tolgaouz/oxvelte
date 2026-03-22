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
        walk_with_parent(&ctx.ast.html, None, ctx);
    }
}

fn walk_with_parent(fragment: &Fragment, parent_name: Option<&str>, ctx: &mut LintContext<'_>) {
    walk_with_parent_nodes(&fragment.nodes, parent_name, ctx);
}

fn walk_with_parent_nodes(nodes: &[TemplateNode], parent_name: Option<&str>, ctx: &mut LintContext<'_>) {
    for node in nodes {
        if let TemplateNode::Element(el) = node {
            if el.name == "script" && !is_allowed_nested_script(parent_name) {
                ctx.diagnostic(
                    "`<script>` should be at the top level of the component, not nested inside markup.",
                    el.span,
                );
            }
            if el.name == "style" && !is_allowed_nested_style(parent_name) {
                ctx.diagnostic(
                    "`<style>` should be at the top level of the component, not nested inside markup.",
                    el.span,
                );
            }
            walk_with_parent_nodes(&el.children, Some(el.name.as_str()), ctx);
        } else {
            walk_block_children(node, parent_name, ctx);
        }
    }
}

/// `<script>` is valid inside `<svelte:head>` (for loading external scripts).
fn is_allowed_nested_script(parent: Option<&str>) -> bool {
    matches!(parent, Some("svelte:head"))
}

/// `<style>` is valid inside `<svg>` elements and `<svelte:head>`.
fn is_allowed_nested_style(parent: Option<&str>) -> bool {
    matches!(parent, Some("svg") | Some("svelte:head"))
}

fn walk_block_children(node: &TemplateNode, parent_name: Option<&str>, ctx: &mut LintContext<'_>) {
    match node {
        TemplateNode::IfBlock(block) => {
            walk_with_parent_nodes(&block.consequent.nodes, parent_name, ctx);
            if let Some(alt) = &block.alternate {
                walk_block_children(alt, parent_name, ctx);
            }
        }
        TemplateNode::EachBlock(block) => {
            walk_with_parent_nodes(&block.body.nodes, parent_name, ctx);
            if let Some(fb) = &block.fallback {
                walk_with_parent_nodes(&fb.nodes, parent_name, ctx);
            }
        }
        TemplateNode::AwaitBlock(block) => {
            if let Some(p) = &block.pending { walk_with_parent_nodes(&p.nodes, parent_name, ctx); }
            if let Some(t) = &block.then { walk_with_parent_nodes(&t.nodes, parent_name, ctx); }
            if let Some(c) = &block.catch { walk_with_parent_nodes(&c.nodes, parent_name, ctx); }
        }
        TemplateNode::KeyBlock(block) => walk_with_parent_nodes(&block.body.nodes, parent_name, ctx),
        TemplateNode::SnippetBlock(block) => walk_with_parent_nodes(&block.body.nodes, parent_name, ctx),
        _ => {}
    }
}
