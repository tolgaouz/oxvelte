//! `svelte/no-target-blank` — disallow `target="_blank"` without `rel="noopener noreferrer"`.

use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};
use crate::linter::{walk_template_nodes, LintContext, Rule};

pub struct NoTargetBlank;

/// Vendor's `/^(?:\w+:|\/\/)/` test: matches a URL with a scheme prefix
/// (`http:`, `data:`, `mailto:`…) or a protocol-relative URL (`//cdn.…`).
/// `\w` in JS regex is `[A-Za-z0-9_]` and the `+` requires at least one
/// such character before the `:`.
fn is_external_url(s: &str) -> bool {
    if s.starts_with("//") {
        return true;
    }
    let mut iter = s.chars();
    let mut saw_word = false;
    while let Some(c) = iter.next() {
        if c == ':' {
            return saw_word;
        }
        if c.is_ascii_alphanumeric() || c == '_' {
            saw_word = true;
        } else {
            return false;
        }
    }
    false
}

/// Joins all *static* text fragments of an attribute value into a single
/// string for whitespace-tokenization. Mustache parts are dropped (vendor
/// only inspects `SvelteLiteral` parts of `rel`).
fn collect_static_words(value: &AttributeValue) -> Vec<String> {
    let mut words = Vec::new();
    let push_text = |words: &mut Vec<String>, text: &str| {
        for w in text.split_whitespace() {
            words.push(w.to_ascii_lowercase());
        }
    };
    match value {
        AttributeValue::Static(s) => push_text(&mut words, s),
        AttributeValue::Concat(parts) => {
            for part in parts {
                if let AttributeValuePart::Static(s) = part {
                    push_text(&mut words, s);
                }
            }
        }
        _ => {}
    }
    words
}

/// First static text fragment, used for the `href` external-URL test.
/// Mirrors vendor's `attr.value[0].type === 'SvelteLiteral'` check.
fn first_static_part(value: &AttributeValue) -> Option<&str> {
    match value {
        AttributeValue::Static(s) => Some(s.as_str()),
        AttributeValue::Concat(parts) => match parts.first() {
            Some(AttributeValuePart::Static(s)) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

impl Rule for NoTargetBlank {
    fn name(&self) -> &'static str {
        "svelte/no-target-blank"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
        let enforce_dynamic = opts
            .and_then(|v| v.get("enforceDynamicLinks"))
            .and_then(|v| v.as_str())
            .unwrap_or("always")
            == "always";
        let allow_referrer = opts
            .and_then(|v| v.get("allowReferrer"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else {
                return;
            };

            // 1. Find a `target="_blank"` attribute. Vendor does not filter by
            //    element name — any element matching this is reported.
            let target_attr = el.attributes.iter().find(|a| match a {
                Attribute::NormalAttribute {
                    name,
                    value: AttributeValue::Static(v),
                    ..
                } => name == "target" && v == "_blank",
                _ => false,
            });
            let Some(target_attr) = target_attr else {
                return;
            };

            // 2. `rel` is "safe" if it contains `noopener` (and `noreferrer`,
            //    unless `allowReferrer`).
            let has_safe_rel = el.attributes.iter().any(|a| {
                let Attribute::NormalAttribute { name, value, .. } = a else {
                    return false;
                };
                if name != "rel" {
                    return false;
                }
                let words = collect_static_words(value);
                words.iter().any(|w| w == "noopener")
                    && (allow_referrer || words.iter().any(|w| w == "noreferrer"))
            });
            if has_safe_rel {
                return;
            }

            // 3. The link is "dangerous" if the `href` is external (vendor's
            //    scheme regex applied to the first static fragment) or, when
            //    `enforceDynamicLinks === "always"`, if the `href` is dynamic.
            //    Dynamic covers: `href={x}`, `<a {href}>` (shorthand collapses
            //    into `NormalAttribute` with an `Expression` value), any
            //    mustache part inside a `Concat` value, or a `bind:href`
            //    directive.
            let mut has_external = false;
            let mut has_dynamic = false;
            for attr in &el.attributes {
                match attr {
                    Attribute::NormalAttribute { name, value, .. } if name == "href" => {
                        if let Some(text) = first_static_part(value) {
                            if is_external_url(text) {
                                has_external = true;
                            }
                        }
                        let value_is_dynamic = matches!(value, AttributeValue::Expression(_))
                            || matches!(value, AttributeValue::Concat(parts)
                                if parts.iter().any(|p| matches!(p, AttributeValuePart::Expression(_))));
                        if value_is_dynamic {
                            has_dynamic = true;
                        }
                    }
                    Attribute::Directive {
                        kind: DirectiveKind::Binding,
                        name,
                        ..
                    } if name == "href" => {
                        has_dynamic = true;
                    }
                    _ => {}
                }
            }
            let has_danger = has_external || (enforce_dynamic && has_dynamic);
            if !has_danger {
                return;
            }

            // 4. Report on the `target` attribute span (vendor's
            //    `context.report({ node })` where `node` is the attribute).
            let target_span = match target_attr {
                Attribute::NormalAttribute { span, .. } => *span,
                _ => el.span,
            };
            ctx.diagnostic(
                "Using target=\"_blank\" without rel=\"noopener noreferrer\" is a security risk.",
                target_span,
            );
        });
    }
}
