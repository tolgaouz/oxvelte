//! `svelte/no-unused-class-name` — disallow class names in the template that are not
//! defined in the `<style>` block.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{TemplateNode, Attribute, AttributeValue, DirectiveKind};
use std::collections::HashSet;
pub struct NoUnusedClassName;

impl Rule for NoUnusedClassName {
    fn name(&self) -> &'static str {
        "svelte/no-unused-class-name"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let allowed_class_names: Vec<String> = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("allowedClassNames"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let (mut allowed_plain, mut allowed_patterns) = (HashSet::new(), Vec::new());
        for name in &allowed_class_names {
            if name.starts_with('/') && name.ends_with('/') && name.len() > 2 {
                allowed_patterns.push(name[1..name.len()-1].to_string());
            } else { allowed_plain.insert(name.clone()); }
        }

        // Collect every `.classname` referenced anywhere in the stylesheet.
        // We walk the CSS once at byte level to isolate each rule's prelude
        // (the chunk of text ending at `{`), parse that prelude through the
        // typed selector AST, and harvest `Component::Class(name)` across
        // every descendant selector list — including `:global(...)` bodies,
        // `:is(...)`, `:where(...)`, `:not(...)`, `:has(...)`, etc. This
        // replaces the old `bytes[i] == b'.'` loop, which also matched
        // spurious dots inside declaration values and string literals.
        let mut css_classes: HashSet<String> = HashSet::new();
        if let Some(style) = &ctx.ast.css {
            collect_css_classes(&style.content, &mut css_classes);
        }

        walk_template_nodes(&ctx.ast.html, &mut |node| {
            if let TemplateNode::Element(el) = node {
                let mut element_classes = Vec::new();

                for attr in &el.attributes {
                    match attr {
                        Attribute::NormalAttribute { name, value, .. } if name == "class" => {
                            if let AttributeValue::Static(val) = value {
                                element_classes.extend(val.split_whitespace().map(String::from));
                            }
                        }
                        Attribute::Directive { kind: DirectiveKind::Class, name: cls_name, .. } => {
                            element_classes.push(cls_name.clone());
                        }
                        _ => {}
                    }
                }

                for cls in &element_classes {
                    if css_classes.contains(cls.as_str()) || allowed_plain.contains(cls.as_str())
                        || allowed_patterns.iter().any(|p| simple_regex_match(p, cls)) { continue; }
                    ctx.diagnostic(format!("Unused class \"{}\".", cls), el.span);
                }
            }
        });
    }
}

/// Collect every `.classname` referenced anywhere in `css` by walking the
/// full cssparser token stream (no hand-rolled tokenizer, no brace tracking,
/// no string/comment handling — `Parser::next` does all of that).
///
/// A class selector tokenizes as `Delim('.')` immediately followed by
/// `Ident(name)`. We flatten nested blocks (`{...}`, `(...)`, `[...]`,
/// `func(...)`) by recursing through `parse_nested_block`, so class names
/// appearing inside `:global(...)`, `:is(...)`, media-rule preludes, or
/// even malformed CSS bodies are all picked up — matching the lenient
/// behavior the old byte scanner provided for the invalid-style fixture.
fn collect_css_classes(css: &str, out: &mut HashSet<String>) {
    let mut input = cssparser::ParserInput::new(css);
    let mut parser = cssparser::Parser::new(&mut input);
    walk_tokens(&mut parser, &mut |tok, prev_is_dot| {
        if prev_is_dot {
            if let cssparser::Token::Ident(name) = tok {
                out.insert(name.as_ref().to_string());
            }
        }
    });
}

fn walk_tokens<F>(parser: &mut cssparser::Parser<'_, '_>, f: &mut F)
where
    F: FnMut(&cssparser::Token<'_>, bool),
{
    let mut prev_is_dot = false;
    loop {
        let token = match parser.next() {
            Ok(t) => t.clone(),
            Err(_) => return,
        };
        f(&token, prev_is_dot);
        prev_is_dot = matches!(&token, cssparser::Token::Delim('.'));
        match &token {
            cssparser::Token::Function(_)
            | cssparser::Token::ParenthesisBlock
            | cssparser::Token::CurlyBracketBlock
            | cssparser::Token::SquareBracketBlock => {
                let _ = parser.parse_nested_block(
                    |inner| -> Result<(), cssparser::ParseError<'_, ()>> {
                        walk_tokens(inner, f);
                        Ok(())
                    },
                );
            }
            _ => {}
        }
    }
}

fn simple_regex_match(pattern: &str, text: &str) -> bool {
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$');
    let inner = pattern.strip_prefix('^').unwrap_or(pattern);
    let inner = inner.strip_suffix('$').unwrap_or(inner);

    if anchored_start {
        return regex_match_inner_impl(inner, text, 0, 0, anchored_end);
    }
    for i in 0..=text.len() {
        if regex_match_inner_impl(inner, text, 0, i, anchored_end) {
            return true;
        }
    }
    false
}

fn regex_match_inner_impl(pattern: &str, text: &str, pi: usize, ti: usize, must_consume_all: bool) -> bool {
    if pi >= pattern.len() {
        return if must_consume_all { ti >= text.len() } else { true };
    }
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();

    if pb[pi] == b'\\' && pi + 1 < pattern.len() {
        let matches_char = |c: u8| -> bool {
            match pb[pi + 1] {
                b'd' => c.is_ascii_digit(),
                b'w' => c.is_ascii_alphanumeric() || c == b'_',
                b's' => c.is_ascii_whitespace(),
                other => c == other,
            }
        };
        if pi + 2 < pattern.len() && pb[pi + 2] == b'{' {
            if let Some(close) = pattern[pi+2..].find('}') {
                let quant = &pattern[pi+3..pi+2+close];
                let (min, max) = if let Some(comma) = quant.find(',') {
                    let mn: usize = quant[..comma].parse().unwrap_or(0);
                    let mx: usize = quant[comma+1..].parse().unwrap_or(mn);
                    (mn, mx)
                } else {
                    let n: usize = quant.parse().unwrap_or(1);
                    (n, n)
                };
                let next_pi = pi + 2 + close + 1;
                let mut count = 0;
                let mut t = ti;
                while count < max && t < tb.len() && matches_char(tb[t]) {
                    count += 1;
                    t += 1;
                    if count >= min && regex_match_inner_impl(pattern, text, next_pi, t, must_consume_all) {
                        return true;
                    }
                }
                return count >= min && regex_match_inner_impl(pattern, text, next_pi, ti + count, must_consume_all);
            }
        }
        if ti < tb.len() && matches_char(tb[ti]) {
            return regex_match_inner_impl(pattern, text, pi + 2, ti + 1, must_consume_all);
        }
        return false;
    }

    if ti < tb.len() && (pb[pi] == tb[ti] || pb[pi] == b'.') {
        return regex_match_inner_impl(pattern, text, pi + 1, ti + 1, must_consume_all);
    }
    false
}
