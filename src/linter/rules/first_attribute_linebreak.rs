//! `svelte/first-attribute-linebreak` — enforce the location of first attribute.
//! 🔧 Fixable

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute};

pub struct FirstAttributeLinebreak;

fn attr_span(attr: &Attribute) -> oxc::span::Span {
    match attr {
        Attribute::NormalAttribute { span, .. } | Attribute::Spread { span } | Attribute::Directive { span, .. } => *span,
    }
}

impl Rule for FirstAttributeLinebreak {
    fn name(&self) -> &'static str { "svelte/first-attribute-linebreak" }
    fn is_fixable(&self) -> bool { true }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref().and_then(|v| v.as_array()).and_then(|arr| arr.first());
        let get_mode = |key, default| opts.and_then(|v| v.get(key)).and_then(|v| v.as_str()).unwrap_or(default).to_string();
        let multiline_mode = get_mode("multiline", "below");
        let singleline_mode = get_mode("singleline", "beside");
        let src = ctx.source;

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            if el.attributes.is_empty() { return; }

            let first_start = attr_span(el.attributes.first().unwrap()).start as usize;
            let last_end = attr_span(el.attributes.last().unwrap()).end as usize;
            let is_single = src[..first_start].matches('\n').count() == src[..last_end].matches('\n').count();
            let mode = if is_single { &singleline_mode } else { &multiline_mode };

            let tag_src = &src[el.span.start as usize..];
            if let Some(name_end) = tag_src.find(|c: char| c.is_whitespace()) {
                let after_name = &tag_src[name_end..];
                let on_new_line = after_name.starts_with('\n') || after_name.starts_with("\r\n");
                if mode == "below" && !on_new_line && !after_name.trim_start().is_empty() {
                    ctx.diagnostic("Expected a linebreak before this attribute.", el.span);
                } else if mode == "beside" && on_new_line {
                    ctx.diagnostic("Expected no linebreak before this attribute.", el.span);
                }
            }
        });
    }
}
