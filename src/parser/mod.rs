//! Svelte file parser. Splits a `.svelte` file into template, script, and style
//! regions, then parses the template with a custom parser and hands script
//! content to `oxc::parser`.

pub mod template;
pub mod serialize;

use oxc_diagnostics::OxcDiagnostic;
use oxc::span::Span;
use crate::ast::*;

#[derive(Debug)]
pub struct ParseResult {
    pub ast: SvelteAst,
    pub errors: Vec<OxcDiagnostic>,
}

/// Parse a `.svelte` source string.
pub fn parse(source: &str) -> ParseResult {
    let mut errors = Vec::new();
    let regions = extract_regions(source);

    let instance = regions.instance.map(|r| Script {
        content: r.content.to_string(), module: false,
        lang: r.lang.map(|s| s.to_string()), span: r.span,
    });
    let module = regions.module.map(|r| Script {
        content: r.content.to_string(), module: true,
        lang: r.lang.map(|s| s.to_string()), span: r.span,
    });
    let css = regions.style.map(|r| Style {
        content: r.content.to_string(),
        lang: r.lang.map(|s| s.to_string()), span: r.span,
    });

    let html = match template::parse_fragment(source) {
        Ok(fragment) => fragment,
        Err(e) => {
            errors.push(e);
            Fragment { nodes: Vec::new(), span: Span::new(0, source.len() as u32) }
        }
    };

    ParseResult { ast: SvelteAst { html, instance, module, css }, errors }
}

// ─── Region extraction ─────────────────────────────────────────────────────

#[derive(Debug)]
struct Region<'a> {
    content: &'a str,
    lang: Option<&'a str>,
    span: Span,
}

#[derive(Debug, Default)]
struct Regions<'a> {
    instance: Option<Region<'a>>,
    module: Option<Region<'a>>,
    style: Option<Region<'a>>,
}

fn extract_regions<'a>(source: &'a str) -> Regions<'a> {
    let mut regions = Regions::default();

    let mut search_from = 0;
    while let Some(open_start) = source[search_from..].find("<script") {
        let open_start = search_from + open_start;
        let after_tag = &source[open_start + 7..];
        let Some(open_end_rel) = after_tag.find('>') else { break };
        let tag_attrs = &after_tag[..open_end_rel];
        let content_start = open_start + 7 + open_end_rel + 1;
        let Some(close_rel) = find_close_tag(&source[content_start..], "script") else { break };
        let content_end = content_start + close_rel;
        let block_end = source[content_end..].find('>').map(|p| content_end + p + 1)
            .unwrap_or(content_end + 9);
        let content = &source[content_start..content_end];
        let lang = extract_attr(tag_attrs, "lang");
        let is_module = tag_attrs.contains("context=\"module\"")
            || tag_attrs.contains("context='module'")
            || tag_attrs.contains("context=module")
            || tag_attrs.split_whitespace().any(|a| a == "module");

        let region = Region { content, lang, span: Span::new(open_start as u32, block_end as u32) };
        if is_module { regions.module = Some(region); } else { regions.instance = Some(region); }
        search_from = block_end;
    }

    if let Some(open_start) = source.find("<style") {
        // Skip <style> inside <svelte:head>
        let before = &source[..open_start];
        let in_svelte_head = before.rfind("<svelte:head").map(|head_start| {
            !source[head_start..open_start].contains("</svelte:head")
        }).unwrap_or(false);
        if in_svelte_head {
            return regions;
        }
        let after_tag = &source[open_start + 6..];
        if let Some(open_end_rel) = after_tag.find('>') {
            let tag_attrs = &after_tag[..open_end_rel];
            let content_start = open_start + 6 + open_end_rel + 1;
            // Find </style> or </style followed by whitespace then >
            if let Some(close_rel) = find_close_tag(&source[content_start..], "style") {
                let content_end = content_start + close_rel;
                // Find the > after </style
                let close_tag_start = content_end;
                let block_end = source[close_tag_start..].find('>').map(|p| close_tag_start + p + 1)
                    .unwrap_or(content_end + 8);
                regions.style = Some(Region {
                    content: &source[content_start..content_end],
                    lang: extract_attr(tag_attrs, "lang"),
                    span: Span::new(open_start as u32, block_end as u32),
                });
            }
        }
    }
    regions
}

/// Find a closing tag like </tagname> or </tagname  \n  > (with whitespace).
/// Returns the byte offset of `</tagname` relative to the input.
fn find_close_tag(source: &str, tag_name: &str) -> Option<usize> {
    let prefix = format!("</{}", tag_name);
    let mut search_from = 0;
    while let Some(pos) = source[search_from..].find(&prefix) {
        let abs_pos = search_from + pos;
        let after = &source[abs_pos + prefix.len()..];
        // Check that next non-whitespace char is >
        let trimmed = after.trim_start();
        if trimmed.starts_with('>') {
            return Some(abs_pos);
        }
        search_from = abs_pos + prefix.len();
    }
    None
}

fn extract_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let pattern = format!("{}={}", name, quote);
        if let Some(start) = attrs.find(&pattern) {
            let value_start = start + pattern.len();
            let value_end = attrs[value_start..].find(quote)?;
            return Some(&attrs[value_start..value_start + value_end]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_file() {
        let r = parse("");
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_none());
    }

    #[test]
    fn test_script_only() {
        let r = parse("<script>let x = 1;</script>");
        assert!(r.errors.is_empty());
        assert_eq!(r.ast.instance.unwrap().content, "let x = 1;");
    }

    #[test]
    fn test_script_lang_ts() {
        let r = parse(r#"<script lang="ts">let x: number = 1;</script>"#);
        let s = r.ast.instance.unwrap();
        assert_eq!(s.lang.as_deref(), Some("ts"));
    }

    #[test]
    fn test_module_script_legacy() {
        let r = parse(r#"<script context="module">export const foo = 1;</script>"#);
        assert!(r.ast.module.is_some());
        assert!(r.ast.instance.is_none());
    }

    #[test]
    fn test_module_script_svelte5() {
        let r = parse("<script module>export const foo = 1;</script>");
        assert!(r.ast.module.is_some());
    }

    #[test]
    fn test_style_block() {
        let r = parse("<style>div { color: red; }</style>");
        assert_eq!(r.ast.css.unwrap().content, "div { color: red; }");
    }

    #[test]
    fn test_full_component() {
        let source = "<script>\n    let count = 0;\n</script>\n\n<button>{count}</button>\n\n<style>\n    button { color: blue; }\n</style>";
        let r = parse(source);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }
}
