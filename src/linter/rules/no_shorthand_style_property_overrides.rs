//! `svelte/no-shorthand-style-property-overrides` — disallow shorthand properties that override related longhand properties.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, DirectiveKind};

pub struct NoShorthandStylePropertyOverrides;

impl Rule for NoShorthandStylePropertyOverrides {
    fn name(&self) -> &'static str {
        "svelte/no-shorthand-style-property-overrides"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let style_directives: Vec<(&str, &oxc::span::Span)> = el.attributes.iter()
                    .filter_map(|a| {
                        if let Attribute::Directive { kind: DirectiveKind::StyleDirective, name, span, .. } = a {
                            Some((name.as_str(), span))
                        } else {
                            None
                        }
                    })
                    .collect();

                for (name, span) in &style_directives {
                    let shorthand = get_shorthand_for(name);
                    if let Some(sh) = shorthand {
                        // Check if the shorthand appears AFTER this longhand
                        for (other_name, other_span) in &style_directives {
                            if *other_name == sh && other_span.start > span.start {
                                ctx.diagnostic(
                                    format!("Shorthand property 'style:{}' overrides 'style:{}'.", other_name, name),
                                    **other_span,
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}

fn get_shorthand_for(property: &str) -> Option<&'static str> {
    match property {
        "border-top-color" | "border-right-color" | "border-bottom-color" | "border-left-color" => Some("border-color"),
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => Some("border-width"),
        "border-top-style" | "border-right-style" | "border-bottom-style" | "border-left-style" => Some("border-style"),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => Some("padding"),
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => Some("margin"),
        "border-top" | "border-right" | "border-bottom" | "border-left" => Some("border"),
        _ => None,
    }
}
