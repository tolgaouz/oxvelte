//! `svelte/system` — internal system rule for Svelte component validation.
//! ⭐ Recommended
//!
//! Checks for common structural issues in Svelte components such as
//! multiple `<script>` or `<style>` blocks (which is handled by the parser)
//! and other basic structural constraints.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::TemplateNode;

pub struct System;

impl Rule for System {
    fn name(&self) -> &'static str {
        "svelte/system"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Check for <script> or <style> elements inside the template body
        // (they should be top-level only).
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                if el.name == "script" {
                    ctx.diagnostic(
                        "`<script>` should be at the top level of the component, not nested inside markup.",
                        el.span,
                    );
                }
                if el.name == "style" {
                    ctx.diagnostic(
                        "`<style>` should be at the top level of the component, not nested inside markup.",
                        el.span,
                    );
                }
            }
        });
    }
}
