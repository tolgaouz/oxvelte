//! `svelte/shorthand-directive` — enforce use of shorthand syntax for directives.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct ShorthandDirective;

impl Rule for ShorthandDirective {
    fn name(&self) -> &'static str {
        "svelte/shorthand-directive"
    }

    fn is_fixable(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let prefer_never = ctx.config.options.as_ref()
            .and_then(|v| v.as_array()).and_then(|arr| arr.first())
            .and_then(|v| v.get("prefer")).and_then(|v| v.as_str()) == Some("never");

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            for attr in &el.attributes {
                let Attribute::Directive { kind, name, span, .. } = attr else { continue };
                if !matches!(kind, DirectiveKind::Binding | DirectiveKind::Class | DirectiveKind::StyleDirective) { continue; }
                let region = &ctx.source[span.start as usize..span.end as usize];
                if prefer_never {
                    if !region.contains('=') { ctx.diagnostic("Expected regular directive syntax.", *span); }
                } else if let Some(eq) = region.find('=') {
                    let v = region[eq + 1..].trim();
                    let expr = if v.starts_with('{') && v.ends_with('}') { &v[1..v.len()-1] } else { v };
                    if expr.trim() == name.as_str() { ctx.diagnostic("Expected shorthand directive.", *span); }
                }
            }
        });
    }
}
