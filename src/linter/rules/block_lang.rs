//! `svelte/block-lang` — enforce or disallow specific `lang` attributes on script/style blocks.
//! 💡

use crate::linter::{LintContext, Rule, Fix};
use oxc::span::Span;

pub struct BlockLang;

/// Build a human-readable description of the allowed langs list.
/// Examples:
///   [null]          → "omitted"
///   ["ts"]          → "\"ts\""
///   ["ts", null]    → "either omitted or \"ts\""
///   ["ts", "typescript", null] → "either omitted or one of \"ts\", \"typescript\""
///   ["ts", "typescript"] → "\"ts\" or \"typescript\""  (2 items, no null)
///   ["ts", "typescript", "js"] → "one of \"ts\", \"typescript\", \"js\""
fn pretty_print_langs(allowed: &[Option<String>]) -> String {
    let has_null = allowed.iter().any(|a| a.is_none());
    let named: Vec<&str> = allowed.iter().filter_map(|a| a.as_deref()).collect();

    match (has_null, named.len()) {
        // Only null allowed
        (true, 0) => "omitted".to_string(),
        // One non-null, with null also allowed
        (true, 1) => format!("either omitted or \"{}\"", named[0]),
        // Multiple non-null, with null also allowed
        (true, _) => {
            let quoted: Vec<String> = named.iter().map(|s| format!("\"{}\"", s)).collect();
            format!("either omitted or one of {}", quoted.join(", "))
        }
        // One non-null, no null
        (false, 1) => format!("\"{}\"", named[0]),
        // Two non-null, no null
        (false, 2) => format!("\"{}\" or \"{}\"", named[0], named[1]),
        // Three+ non-null, no null
        (false, _) => {
            let quoted: Vec<String> = named.iter().map(|s| format!("\"{}\"", s)).collect();
            format!("one of {}", quoted.join(", "))
        }
    }
}

fn parse_langs(opts: Option<&serde_json::Value>, key: &str) -> Option<Vec<Option<String>>> {
    opts.and_then(|o| o.get(key)).and_then(|v| {
        if let Some(arr) = v.as_array() {
            Some(arr.iter().map(|v| v.as_str().map(String::from)).collect())
        } else {
            v.as_str().map(|s| vec![Some(s.to_string())])
        }
    })
}

fn check_block_lang(
    tag: &str, span: Span, block_lang: Option<&str>,
    allowed: &[Option<String>], source: &str, ctx: &mut LintContext<'_>,
) {
    let lang = block_lang.map(|l| l.to_lowercase());
    let lang_ref = lang.as_deref();
    let allowed_lower: Vec<Option<String>> = allowed.iter()
        .map(|a| a.as_deref().map(|s| s.to_lowercase())).collect();
    if allowed_lower.iter().any(|a| a.as_deref() == lang_ref) { return; }

    let msg = format!("The lang attribute of the <{}> block should be {}.", tag, pretty_print_langs(allowed));
    let src = &source[span.start as usize..span.end as usize];
    let replacement = match (allowed.iter().find_map(|a| a.as_deref()), block_lang) {
        (Some(target), Some(l)) => src.replacen(&format!("lang=\"{}\"", l), &format!("lang=\"{}\"", target), 1),
        (Some(target), None) => src.replacen(&format!("<{}", tag), &format!("<{} lang=\"{}\"", tag, target), 1),
        (None, Some(l)) => {
            let with_space = format!(" lang=\"{}\"", l);
            if src.contains(&with_space) { src.replacen(&with_space, "", 1) }
            else { src.replacen(&format!("lang=\"{}\"", l), "", 1) }
        }
        (None, None) => src.to_string(),
    };
    ctx.diagnostic_with_fix(msg, span, Fix { span, replacement });
}

impl Rule for BlockLang {
    fn name(&self) -> &'static str {
        "svelte/block-lang"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        let script_langs = parse_langs(opts, "script");
        let style_langs = parse_langs(opts, "style");
        let enforce_script = opts.and_then(|o| o.get("enforceScriptPresent"))
            .and_then(|v| v.as_bool()).unwrap_or(false);
        let enforce_style = opts.and_then(|o| o.get("enforceStylePresent"))
            .and_then(|v| v.as_bool()).unwrap_or(false);

        // Script block checks
        if enforce_script && ctx.ast.instance.is_none() && ctx.ast.module.is_none() {
            let desc = script_langs.as_ref().map_or("omitted".to_string(), |a| pretty_print_langs(a));
            ctx.diagnostic(
                format!("The <script> block should be present and its lang attribute should be {}.", desc),
                Span::new(0, 0),
            );
        }
        if let Some(allowed) = &script_langs {
            for script in [&ctx.ast.instance, &ctx.ast.module].iter().filter_map(|s| s.as_ref()) {
                check_block_lang("script", script.span, script.lang.as_deref(), allowed, ctx.source, ctx);
            }
        }

        // Style block checks
        if enforce_style && ctx.ast.css.is_none() {
            let desc = style_langs.as_ref().map_or("omitted".to_string(), |a| pretty_print_langs(a));
            ctx.diagnostic(
                format!("The <style> block should be present and its lang attribute should be {}.", desc),
                Span::new(0, 0),
            );
        }
        if let Some(allowed) = &style_langs {
            if let Some(style) = &ctx.ast.css {
                check_block_lang("style", style.span, style.lang.as_deref(), allowed, ctx.source, ctx);
            }
        }
    }
}
