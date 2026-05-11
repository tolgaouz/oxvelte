//! Svelte file parser. Splits a `.svelte` file into template, script, and style
//! regions, then parses the template with a custom parser and hands script
//! content to `oxc::parser`.

pub mod css;
pub mod expression;
pub(crate) mod scanner;
pub mod selector;
pub mod serialize;
pub mod template;

use crate::ast::*;
use oxc::allocator::Allocator;
use oxc::span::Span;
use oxc_diagnostics::OxcDiagnostic;
use scanner::{SvelteScanner, TokenKind};
use std::marker::PhantomData;

#[derive(Debug)]
pub struct ParseResult<'a> {
    pub ast: SvelteAst<'a>,
    pub errors: Vec<OxcDiagnostic>,
}

/// Parse a `.svelte` source string. The supplied `allocator` owns any
/// pre-parsed template-expression AST nodes attached to the returned
/// `SvelteAst` — it must outlive the result.
pub fn parse<'a>(source: &'a str, allocator: &'a Allocator) -> ParseResult<'a> {
    // Match Svelte's `Parser` constructor: trailing whitespace on the
    // template is dropped before parsing. Spans are byte offsets into the
    // trimmed view (same byte positions as the original up to the trim
    // point); the serializer reports `Root.end` from the original
    // `source.len()` to match vendor's `this.root.end = template.length`.
    let trimmed = source.trim_end();
    let mut regions = extract_regions(trimmed);
    let mut errors = std::mem::take(&mut regions.errors);

    let instance = regions.instance.map(|r| Script {
        content: r.content.to_string(),
        module: false,
        lang: r.lang.map(|s| s.to_string()),
        strict_events: r.strict_events,
        span: r.span,
        attrs_span: r.attrs_span,
        content_span: r.content_span,
    });
    let module = regions.module.map(|r| Script {
        content: r.content.to_string(),
        module: true,
        lang: r.lang.map(|s| s.to_string()),
        strict_events: r.strict_events,
        span: r.span,
        attrs_span: r.attrs_span,
        content_span: r.content_span,
    });
    let css = regions.style.map(|r| Style {
        content: r.content.to_string(),
        lang: r.lang.map(|s| s.to_string()),
        span: r.span,
        attrs_span: r.attrs_span,
        content_span: r.content_span,
    });

    let (mut html, template_errors) = template::parse_fragment_with_errors(trimmed, allocator);
    errors.extend(template_errors);

    // Post-pass: parse every attribute expression's text into a typed AST so
    // rules can pattern-match on `oxc::ast::Expression` instead of substring-
    // searching the raw text. Doing this here (rather than inside the
    // template parser) sidesteps a borrow-checker false positive — by this
    // point the template AST is fully owned and we just need `&'a Allocator`.
    populate_attribute_expression_asts(&mut html, allocator);

    ParseResult {
        ast: SvelteAst {
            html,
            instance,
            module,
            css,
            _phantom: PhantomData,
        },
        errors,
    }
}

/// Walks every `Element` in the parsed fragment and fills in
/// `AttributeMeta::expression_ast` (single-expression attributes) and
/// `AttributePartMeta::expression_ast` (mustache parts of `Concat` values).
fn populate_attribute_expression_asts<'a>(
    fragment: &mut crate::ast::Fragment<'a>,
    allocator: &'a oxc::allocator::Allocator,
) {
    use crate::ast::{Attribute, AttributeValue, AttributeValuePart, TemplateNode};

    fn parse_text<'a>(
        text: &str,
        allocator: &'a oxc::allocator::Allocator,
    ) -> Option<&'a oxc::ast::ast::Expression<'a>> {
        use oxc::allocator::CloneIn;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let result = crate::parser::expression::parse_template_expression(trimmed, allocator);
        if !result.errors.is_empty() {
            return None;
        }
        let expr = crate::parser::expression::unwrap_template_expression(&result)?;
        Some(allocator.alloc(expr.clone_in(allocator)))
    }

    fn walk<'a>(nodes: &mut [TemplateNode<'a>], allocator: &'a oxc::allocator::Allocator) {
        for node in nodes {
            match node {
                TemplateNode::Element(el) => {
                    for (idx, attr) in el.attributes.iter().enumerate() {
                        let Some(meta) = el.attribute_meta.get_mut(idx) else {
                            continue;
                        };
                        let expr_text = match attr {
                            Attribute::NormalAttribute {
                                value: AttributeValue::Expression(s),
                                ..
                            } => Some(s.as_str()),
                            Attribute::Directive {
                                value: AttributeValue::Expression(s),
                                ..
                            } => Some(s.as_str()),
                            _ => None,
                        };
                        if let Some(text) = expr_text {
                            meta.expression_ast = parse_text(text, allocator);
                        }
                        // Concat parts.
                        let concat_parts = match attr {
                            Attribute::NormalAttribute {
                                value: AttributeValue::Concat(parts),
                                ..
                            } => Some(parts),
                            Attribute::Directive {
                                value: AttributeValue::Concat(parts),
                                ..
                            } => Some(parts),
                            _ => None,
                        };
                        if let Some(parts) = concat_parts {
                            for (part_idx, part) in parts.iter().enumerate() {
                                let Some(part_meta) = meta.parts.get_mut(part_idx) else {
                                    continue;
                                };
                                if let AttributeValuePart::Expression(s) = part {
                                    part_meta.expression_ast = parse_text(s, allocator);
                                }
                            }
                        }
                    }
                    walk(&mut el.children, allocator);
                }
                TemplateNode::IfBlock(b) => {
                    walk(&mut b.consequent.nodes, allocator);
                    if let Some(alt) = &mut b.alternate {
                        walk(std::slice::from_mut(alt.as_mut()), allocator);
                    }
                }
                TemplateNode::EachBlock(b) => {
                    walk(&mut b.body.nodes, allocator);
                    if let Some(fb) = &mut b.fallback {
                        walk(&mut fb.nodes, allocator);
                    }
                }
                TemplateNode::AwaitBlock(b) => {
                    if let Some(p) = &mut b.pending {
                        walk(&mut p.nodes, allocator);
                    }
                    if let Some(t) = &mut b.then {
                        walk(&mut t.nodes, allocator);
                    }
                    if let Some(c) = &mut b.catch {
                        walk(&mut c.nodes, allocator);
                    }
                }
                TemplateNode::KeyBlock(b) => walk(&mut b.body.nodes, allocator),
                TemplateNode::SnippetBlock(b) => walk(&mut b.body.nodes, allocator),
                _ => {}
            }
        }
    }

    walk(&mut fragment.nodes, allocator);
}

// ─── Region extraction ─────────────────────────────────────────────────────

#[derive(Debug)]
struct Region<'a> {
    content: &'a str,
    lang: Option<&'a str>,
    strict_events: bool,
    span: Span,
    attrs_span: Span,
    content_span: Span,
}

#[derive(Debug, Default)]
struct Regions<'a> {
    instance: Option<Region<'a>>,
    module: Option<Region<'a>>,
    style: Option<Region<'a>>,
    errors: Vec<OxcDiagnostic>,
}

fn extract_regions<'a>(source: &'a str) -> Regions<'a> {
    let mut regions = Regions::default();

    let mut element_stack: Vec<&str> = Vec::new();
    let mut block_depth = 0usize;
    for token in SvelteScanner::new(source) {
        match token.kind {
            TokenKind::StartTag {
                name,
                attrs,
                self_closing,
            } => {
                let _ = attrs;
                if !self_closing && should_track_element(name) {
                    element_stack.push(name);
                }
            }
            TokenKind::EndTag { name } => {
                if let Some(idx) = element_stack
                    .iter()
                    .rposition(|open| open.eq_ignore_ascii_case(name))
                {
                    element_stack.truncate(idx);
                }
            }
            TokenKind::RawRegion {
                name,
                attrs,
                attrs_span,
                content,
                content_span,
                closed,
            } => {
                if block_depth > 0
                    || !element_stack.is_empty()
                    || element_stack
                        .iter()
                        .any(|open| open.eq_ignore_ascii_case("svelte:head"))
                {
                    continue;
                }

                if name.eq_ignore_ascii_case("script") {
                    if !closed {
                        regions
                            .errors
                            .push(OxcDiagnostic::error("Unclosed top-level script block"));
                    }
                    let context_attr = find_attr(attrs, "context");
                    if !matches!(context_attr, None | Some(Some("module"))) {
                        regions.errors.push(OxcDiagnostic::error(
                            "If the context attribute is supplied, its value must be \"module\"",
                        ));
                    }
                    let module_attr = find_attr(attrs, "module");
                    if matches!(module_attr, Some(Some(_))) {
                        regions.errors.push(OxcDiagnostic::error(
                            "If the `module` attribute is supplied, it must be a boolean attribute",
                        ));
                    }
                    for reserved in ["server", "client", "worker", "test", "default"] {
                        if find_attr(attrs, reserved).is_some() {
                            regions.errors.push(OxcDiagnostic::error(format!(
                                "The `{reserved}` attribute is reserved and cannot be used"
                            )));
                        }
                    }
                    let is_module = module_attr == Some(None)
                        || context_attr
                            .is_some_and(|value| value.is_some_and(|value| value == "module"));
                    let region = Region {
                        content,
                        lang: extract_attr(attrs, "lang"),
                        strict_events: has_bool_attr(attrs, "strictEvents"),
                        span: token.span,
                        attrs_span,
                        content_span,
                    };

                    if is_module {
                        if regions.module.is_some() {
                            regions.errors.push(OxcDiagnostic::error(
                                "Duplicate top-level module script block",
                            ));
                        } else {
                            regions.module = Some(region);
                        }
                    } else if regions.instance.is_some() {
                        regions.errors.push(OxcDiagnostic::error(
                            "Duplicate top-level instance script block",
                        ));
                    } else {
                        regions.instance = Some(region);
                    }
                } else if name.eq_ignore_ascii_case("style") {
                    if !closed {
                        regions
                            .errors
                            .push(OxcDiagnostic::error("Unclosed top-level style block"));
                    }
                    let region = Region {
                        content,
                        lang: extract_attr(attrs, "lang"),
                        strict_events: false,
                        span: token.span,
                        attrs_span,
                        content_span,
                    };
                    if regions.style.is_some() {
                        regions
                            .errors
                            .push(OxcDiagnostic::error("Duplicate top-level style block"));
                    } else {
                        regions.style = Some(region);
                    }
                }
            }
            TokenKind::Text(text) | TokenKind::HtmlComment(text) => {
                let _ = text;
            }
            TokenKind::Mustache { expression } => {
                let _ = expression;
            }
            TokenKind::BlockStart {
                keyword,
                expression,
            } => {
                let _ = (keyword, expression);
                if element_stack.is_empty() && is_structural_block_keyword(keyword) {
                    block_depth += 1;
                }
            }
            TokenKind::BlockContinuation {
                keyword,
                expression,
            } => {
                let _ = (keyword, expression);
            }
            TokenKind::BlockEnd { keyword } => {
                if is_structural_block_keyword(keyword) {
                    block_depth = block_depth.saturating_sub(1);
                }
            }
        }
    }

    regions
}

/// True iff `attrs` contains a whole-token attribute named `name`, either as a
/// bare boolean (`<script strictEvents>`), with a value (`strictEvents="true"`,
/// `strictEvents={x}`), or self-close-adjacent (`strictEvents/>`).
fn has_bool_attr(attrs: &str, name: &str) -> bool {
    find_attr(attrs, name).is_some()
}

fn extract_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    find_attr(attrs, name).flatten()
}

fn find_attr<'a>(attrs: &'a str, name: &str) -> Option<Option<&'a str>> {
    let bytes = attrs.as_bytes();
    let mut pos = 0;
    while pos < attrs.len() {
        while pos < attrs.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b'/') {
            pos += 1;
        }
        let name_start = pos;
        while pos < attrs.len() {
            let ch = bytes[pos];
            if ch.is_ascii_whitespace() || ch == b'=' || ch == b'/' || ch == b'>' {
                break;
            }
            pos += 1;
        }
        if name_start == pos {
            pos += 1;
            continue;
        }
        let attr_name = &attrs[name_start..pos];
        while pos < attrs.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let value = if pos < attrs.len() && bytes[pos] == b'=' {
            pos += 1;
            while pos < attrs.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            Some(read_attr_value(attrs, &mut pos))
        } else {
            None
        };

        if attr_name == name {
            return Some(value);
        }
    }
    None
}

fn read_attr_value<'a>(attrs: &'a str, pos: &mut usize) -> &'a str {
    if *pos >= attrs.len() {
        return "";
    }
    let bytes = attrs.as_bytes();
    match bytes[*pos] {
        b'\'' | b'"' => {
            let quote = bytes[*pos];
            *pos += 1;
            let start = *pos;
            while *pos < attrs.len() {
                match bytes[*pos] {
                    b'{' => {
                        let expr_end = scanner::find_expression_end(attrs, *pos + 1);
                        *pos = (expr_end + 1).min(attrs.len());
                    }
                    ch if ch == quote => break,
                    _ => *pos += 1,
                }
            }
            let end = *pos;
            if *pos < attrs.len() {
                *pos += 1;
            }
            &attrs[start..end]
        }
        b'{' => {
            *pos += 1;
            let start = *pos;
            *pos = scanner::find_expression_end(attrs, *pos);
            let end = *pos;
            if *pos < attrs.len() {
                *pos += 1;
            }
            &attrs[start..end]
        }
        _ => {
            let start = *pos;
            while *pos < attrs.len() {
                let ch = bytes[*pos];
                if ch.is_ascii_whitespace()
                    || ch == b'>'
                    || (ch == b'/' && attrs[*pos..].starts_with("/>") && *pos > start)
                {
                    break;
                }
                *pos += 1;
            }
            &attrs[start..*pos]
        }
    }
}

fn should_track_element(name: &str) -> bool {
    !name.starts_with('!') && !scanner::is_html_void_element(name)
}

fn is_structural_block_keyword(keyword: &str) -> bool {
    matches!(keyword, "if" | "each" | "await" | "key" | "snippet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_file() {
        let alloc = Allocator::default();
        let r = parse("", &alloc);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_none());
    }

    #[test]
    fn test_script_only() {
        let alloc = Allocator::default();
        let r = parse("<script>let x = 1;</script>", &alloc);
        assert!(r.errors.is_empty());
        assert_eq!(r.ast.instance.unwrap().content, "let x = 1;");
    }

    #[test]
    fn test_script_lang_ts() {
        let alloc = Allocator::default();
        let r = parse(r#"<script lang="ts">let x: number = 1;</script>"#, &alloc);
        let s = r.ast.instance.unwrap();
        assert_eq!(s.lang.as_deref(), Some("ts"));
    }

    #[test]
    fn test_module_script_legacy() {
        let alloc = Allocator::default();
        let r = parse(
            r#"<script context="module">export const foo = 1;</script>"#,
            &alloc,
        );
        assert!(r.ast.module.is_some());
        assert!(r.ast.instance.is_none());
    }

    #[test]
    fn test_script_context_invalid_value_reports_diagnostic() {
        let alloc = Allocator::default();
        for source in [
            r#"<script context>let x = 1;</script>"#,
            r#"<script context="foo">let x = 1;</script>"#,
            r#"<script context={"module"}>let x = 1;</script>"#,
        ] {
            let r = parse(source, &alloc);
            assert!(
                r.errors.iter().any(|error| error
                    .to_string()
                    .contains("If the context attribute is supplied")),
                "{source}: {:?}",
                r.errors
            );
        }
    }

    #[test]
    fn test_script_reserved_and_module_attribute_diagnostics() {
        let alloc = Allocator::default();

        let r = parse(r#"<script module="x">let x = 1;</script>"#, &alloc);
        assert!(
            r.errors
                .iter()
                .any(|error| error.to_string().contains("must be a boolean attribute")),
            "{:?}",
            r.errors
        );

        let r = parse(r#"<script server>let x = 1;</script>"#, &alloc);
        assert!(
            r.errors.iter().any(|error| error
                .to_string()
                .contains("The `server` attribute is reserved")),
            "{:?}",
            r.errors
        );

        let r = parse(
            r#"<script module context="module">let x = 1;</script>"#,
            &alloc,
        );
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.ast.module.unwrap().content, "let x = 1;");
    }

    #[test]
    fn test_module_script_svelte5() {
        let alloc = Allocator::default();
        let r = parse("<script module>export const foo = 1;</script>", &alloc);
        assert!(r.ast.module.is_some());
    }

    #[test]
    fn test_style_block() {
        let alloc = Allocator::default();
        let r = parse("<style>div { color: red; }</style>", &alloc);
        assert_eq!(r.ast.css.unwrap().content, "div { color: red; }");
    }

    #[test]
    fn test_style_tag_attribute_expression_may_contain_gt() {
        let alloc = Allocator::default();
        let source = "<style lang={foo > bar}>main { color: red; }</style>";
        let r = parse(source, &alloc);
        let style = r.ast.css.expect("expected style");
        assert_eq!(style.lang.as_deref(), Some("foo > bar"));
        assert_eq!(style.content, "main { color: red; }");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_full_component() {
        let alloc = Allocator::default();
        let source = "<script>\n    let count = 0;\n</script>\n\n<button>{count}</button>\n\n<style>\n    button { color: blue; }\n</style>";
        let r = parse(source, &alloc);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    #[test]
    fn test_tag_name_prefix_is_not_script() {
        let alloc = Allocator::default();
        let r = parse("<scripture>text</scripture>", &alloc);
        assert!(r.ast.instance.is_none());
        match &r.ast.html.nodes[0] {
            TemplateNode::Element(el) => assert_eq!(el.name, "scripture"),
            other => panic!("expected scripture element, got {other:?}"),
        }
    }

    #[test]
    fn test_script_inside_head_is_not_instance() {
        let alloc = Allocator::default();
        let source = r#"<svelte:head><script src="/analytics.js"></script></svelte:head><script>let x = 1;</script>"#;
        let r = parse(source, &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let x = 1;");
    }

    #[test]
    fn test_style_after_head_style_is_extracted() {
        let alloc = Allocator::default();
        let source =
            "<svelte:head><style></style></svelte:head><style>main { color: red; }</style>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.css.unwrap().content, "main { color: red; }");
    }

    #[test]
    fn test_script_in_html_comment_is_not_instance() {
        let alloc = Allocator::default();
        let r = parse("<!-- <script>bad</script> --><p>ok</p>", &alloc);
        assert!(r.ast.instance.is_none());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_script_in_attribute_is_not_instance() {
        let alloc = Allocator::default();
        let source = r#"<div data-example="<script>bad</script>"></div>"#;
        let r = parse(source, &alloc);
        assert!(r.ast.instance.is_none());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_script_in_child_content_is_not_instance() {
        let alloc = Allocator::default();
        let source = "<div><script>bad</script></div>";
        let r = parse(source, &alloc);
        assert!(r.ast.instance.is_none());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_script_inside_block_is_not_instance() {
        let alloc = Allocator::default();
        let source = "{#if ok}<script>if (ok) { run(); }</script>{/if}";
        let r = parse(source, &alloc);
        assert!(r.ast.instance.is_none());
        match &r.ast.html.nodes[0] {
            TemplateNode::IfBlock(block) => match &block.consequent.nodes[0] {
                TemplateNode::Element(element) => {
                    assert_eq!(element.name, "script");
                    match &element.children[0] {
                        TemplateNode::Text(text) => assert_eq!(text.data, "if (ok) { run(); }"),
                        other => panic!("expected script text, got {other:?}"),
                    }
                }
                other => panic!("expected script element, got {other:?}"),
            },
            other => panic!("expected if block, got {other:?}"),
        }
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_special_tag_before_script_does_not_hide_top_level_script() {
        let alloc = Allocator::default();
        let source = "{@html content}\n<script>let ok = true;</script>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let ok = true;");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_style_in_html_comment_is_not_css() {
        let alloc = Allocator::default();
        let r = parse("<!-- <style>.bad {}</style> --><p>ok</p>", &alloc);
        assert!(r.ast.css.is_none());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_style_in_attribute_is_not_css() {
        let alloc = Allocator::default();
        let source = r#"<div data-example="<style>.bad {}</style>"></div>"#;
        let r = parse(source, &alloc);
        assert!(r.ast.css.is_none());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_script_inside_raw_text_element_is_not_instance() {
        let alloc = Allocator::default();
        let source = "<textarea><script>bad</script></textarea><script>let ok = true;</script>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let ok = true;");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_block_inside_textarea_reports_diagnostic_without_hiding_script() {
        let alloc = Allocator::default();
        let source = "<textarea>{#if x}</textarea><script>let ok = true;</script>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let ok = true;");
        assert!(r.errors.iter().any(|error| error
            .to_string()
            .contains("block cannot be inside <textarea>")));
    }

    #[test]
    fn test_style_inside_raw_text_element_is_not_css() {
        let alloc = Allocator::default();
        let source = "<textarea><style>.bad {}</style></textarea><style>.ok {}</style>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.css.unwrap().content, ".ok {}");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_style_inside_block_is_not_css() {
        let alloc = Allocator::default();
        let source = "{#if ok}<style>.scoped { color: red; }</style>{/if}";
        let r = parse(source, &alloc);
        assert!(r.ast.css.is_none());
        match &r.ast.html.nodes[0] {
            TemplateNode::IfBlock(block) => match &block.consequent.nodes[0] {
                TemplateNode::Element(element) => {
                    assert_eq!(element.name, "style");
                    match &element.children[0] {
                        TemplateNode::Text(text) => {
                            assert_eq!(text.data, ".scoped { color: red; }")
                        }
                        other => panic!("expected style text, got {other:?}"),
                    }
                }
                other => panic!("expected style element, got {other:?}"),
            },
            other => panic!("expected if block, got {other:?}"),
        }
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_special_tag_before_style_does_not_hide_top_level_style() {
        let alloc = Allocator::default();
        let source = "{@debug value}\n<style>.ok { color: red; }</style>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.css.unwrap().content, ".ok { color: red; }");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_script_tag_attribute_expression_may_contain_gt() {
        let alloc = Allocator::default();
        let source = "<script lang={foo > bar}>let ok = true;</script>";
        let r = parse(source, &alloc);
        let script = r.ast.instance.expect("expected instance script");
        assert_eq!(script.lang.as_deref(), Some("foo > bar"));
        assert_eq!(script.content, "let ok = true;");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_quoted_script_attr_expression_may_contain_matching_quote() {
        let alloc = Allocator::default();
        let source = r#"<script lang="{foo(">")}">let ok = true;</script>"#;
        let r = parse(source, &alloc);
        let script = r.ast.instance.expect("expected instance script");
        assert_eq!(script.lang.as_deref(), Some(r#"{foo(">")}"#));
        assert_eq!(script.content, "let ok = true;");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_script_skip_ignores_close_text_in_opening_attribute() {
        let alloc = Allocator::default();
        let source = r#"<script data-close="</script>">let ok = true;</script><p>after</p>"#;
        let r = parse(source, &alloc);
        let script = r.ast.instance.expect("expected instance script");
        assert_eq!(script.content, "let ok = true;");
        assert_eq!(r.ast.html.nodes.len(), 1);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_duplicate_top_level_scripts_report_diagnostic() {
        let alloc = Allocator::default();
        let source = "<script>let first = true;</script><script>let second = true;</script>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let first = true;");
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn test_duplicate_top_level_styles_report_diagnostic() {
        let alloc = Allocator::default();
        let source = "<style>.first {}</style><style>.second {}</style>";
        let r = parse(source, &alloc);
        assert_eq!(r.ast.css.unwrap().content, ".first {}");
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn test_unclosed_top_level_script_reports_diagnostic() {
        let alloc = Allocator::default();
        let r = parse("<script>let x = 1;", &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let x = 1;");
        assert!(!r.errors.is_empty());
    }

    #[test]
    fn test_script_close_tag_is_case_insensitive() {
        let alloc = Allocator::default();
        let r = parse("<SCRIPT>let x = 1;</script>", &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let x = 1;");
        assert!(r.ast.html.nodes.is_empty());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_uppercase_void_element_does_not_hide_top_level_script() {
        let alloc = Allocator::default();
        let r = parse("<BR><script>let x = 1;</script>", &alloc);
        assert_eq!(r.ast.instance.unwrap().content, "let x = 1;");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_unclosed_template_block_reports_diagnostic() {
        let alloc = Allocator::default();
        let r = parse("{#if visible}<p>hello", &alloc);
        assert!(!r.errors.is_empty());
    }
}
