//! `svelte/block-lang` — enforce or disallow specific `lang` attributes on script/style blocks.
//! 💡

use crate::linter::{LintContext, Rule, Fix};
use oxc::span::Span;

pub struct BlockLang;

impl Rule for BlockLang {
    fn name(&self) -> &'static str {
        "svelte/block-lang"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let opts = ctx.config.options.as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());

        // Parse script allowed langs — can be a string or an array
        let script_langs: Option<Vec<Option<String>>> = opts
            .and_then(|o| o.get("script"))
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(arr.iter().map(|v| v.as_str().map(String::from)).collect())
                } else if let Some(s) = v.as_str() {
                    Some(vec![Some(s.to_string())])
                } else {
                    None
                }
            });

        // Parse style allowed langs — can be a string or an array
        let style_langs: Option<Vec<Option<String>>> = opts
            .and_then(|o| o.get("style"))
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(arr.iter().map(|v| v.as_str().map(String::from)).collect())
                } else if let Some(s) = v.as_str() {
                    Some(vec![Some(s.to_string())])
                } else {
                    None
                }
            });

        // enforceScriptPresent: if true, script block must exist (but no lang requirement)
        let enforce_script_present = opts
            .and_then(|o| o.get("enforceScriptPresent"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // enforceStylePresent
        let enforce_style_present = opts
            .and_then(|o| o.get("enforceStylePresent"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Check script blocks
        if let Some(allowed) = &script_langs {
            // We have explicit allowed langs for script
            for script in [&ctx.ast.instance, &ctx.ast.module].iter().filter_map(|s| s.as_ref()) {
                let lang = script.lang.as_deref();
                if !allowed.iter().any(|a| a.as_deref() == lang) {
                    // Find the first allowed lang string for the suggestion
                    let first_allowed = allowed.first().and_then(|a| a.as_deref()).unwrap_or("javascript");
                    let msg = format!("The lang attribute of the <script> block should be \"{}\".", first_allowed);
                    // Build suggestion: replace script tag with correct lang
                    let source = &ctx.source[script.span.start as usize..script.span.end as usize];
                    let replacement = if let Some(l) = lang {
                        source.replacen(&format!("lang=\"{}\"", l), &format!("lang=\"{}\"", first_allowed), 1)
                    } else {
                        source.replacen("<script", &format!("<script lang=\"{}\"", first_allowed), 1)
                    };
                    ctx.diagnostic_with_fix(
                        msg,
                        script.span,
                        Fix { span: script.span, replacement },
                    );
                }
            }
        } else if enforce_script_present {
            // Only enforce that script is present, no lang requirement
            if ctx.ast.instance.is_none() && ctx.ast.module.is_none() {
                ctx.diagnostic(
                    "A <script> block is required.",
                    Span::new(0, 0),
                );
            }
        } else {
            // Default behavior: require lang attribute on script blocks
            if let Some(script) = &ctx.ast.instance {
                if script.lang.is_none() {
                    ctx.diagnostic(
                        "Script block should specify a `lang` attribute (e.g. `lang=\"ts\"`).",
                        script.span,
                    );
                }
            }
            if let Some(module) = &ctx.ast.module {
                if module.lang.is_none() {
                    ctx.diagnostic(
                        "Module script block should specify a `lang` attribute (e.g. `lang=\"ts\"`).",
                        module.span,
                    );
                }
            }
        }

        // Check style blocks
        if let Some(allowed) = &style_langs {
            if let Some(style) = &ctx.ast.css {
                let lang = style.lang.as_deref();
                if !allowed.iter().any(|a| a.as_deref() == lang) {
                    let first_allowed = allowed.first().and_then(|a| a.as_deref());
                    let target = match first_allowed {
                        Some(l) => format!("\"{}\"", l),
                        None => "null (no lang attribute)".to_string(),
                    };
                    ctx.diagnostic(
                        format!("The lang attribute of the <style> block should be {}.", target),
                        style.span,
                    );
                }
            }
        } else if enforce_style_present {
            if ctx.ast.css.is_none() {
                ctx.diagnostic(
                    "A <style> block is required.",
                    Span::new(0, 0),
                );
            }
        } else {
            // Default behavior
            if let Some(style) = &ctx.ast.css {
                if style.lang.is_none() {
                    ctx.diagnostic(
                        "Style block should specify a `lang` attribute (e.g. `lang=\"scss\"`).",
                        style.span,
                    );
                }
            }
        }
    }
}
