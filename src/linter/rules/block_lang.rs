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

        // -----------------------------------------------------------------------
        // Script block checks
        // -----------------------------------------------------------------------

        // enforceScriptPresent is independent — always check it if enabled.
        if enforce_script_present {
            if ctx.ast.instance.is_none() && ctx.ast.module.is_none() {
                let lang_desc = if let Some(ref allowed) = script_langs {
                    pretty_print_langs(allowed)
                } else {
                    "omitted".to_string()
                };
                ctx.diagnostic(
                    format!("The <script> block should be present and its lang attribute should be {}.", lang_desc),
                    Span::new(0, 0),
                );
            }
        }

        // Lang check: only run when script option is explicitly configured.
        if let Some(allowed) = &script_langs {
            for script in [&ctx.ast.instance, &ctx.ast.module].iter().filter_map(|s| s.as_ref()) {
                // Case-insensitive comparison
                let lang = script.lang.as_deref().map(|l| l.to_lowercase());
                let lang_ref = lang.as_deref();
                let allowed_lower: Vec<Option<String>> = allowed.iter()
                    .map(|a| a.as_deref().map(|s| s.to_lowercase()))
                    .collect();

                if !allowed_lower.iter().any(|a| a.as_deref() == lang_ref) {
                    let pretty = pretty_print_langs(allowed);
                    let msg = format!("The lang attribute of the <script> block should be {}.", pretty);

                    // Build a single suggestion: fix to the first non-null allowed lang,
                    // or remove lang if the first allowed is null.
                    let source = &ctx.source[script.span.start as usize..script.span.end as usize];
                    let first_allowed = allowed.iter().find_map(|a| a.as_deref());
                    let replacement = if let Some(target_lang) = first_allowed {
                        if let Some(l) = script.lang.as_deref() {
                            source.replacen(&format!("lang=\"{}\"", l), &format!("lang=\"{}\"", target_lang), 1)
                        } else {
                            source.replacen("<script", &format!("<script lang=\"{}\"", target_lang), 1)
                        }
                    } else {
                        // null-only allowed: remove the lang attribute
                        if let Some(l) = script.lang.as_deref() {
                            // Remove ` lang="..."` (with leading space)
                            let with_space = format!(" lang=\"{}\"", l);
                            if source.contains(&with_space) {
                                source.replacen(&with_space, "", 1)
                            } else {
                                source.replacen(&format!("lang=\"{}\"", l), "", 1)
                            }
                        } else {
                            source.to_string()
                        }
                    };

                    ctx.diagnostic_with_fix(
                        msg,
                        script.span,
                        Fix { span: script.span, replacement },
                    );
                }
            }
        }
        // If script_langs is None (not configured), no lang diagnostics — vendor treats it as a no-op.

        // -----------------------------------------------------------------------
        // Style block checks
        // -----------------------------------------------------------------------

        // enforceStylePresent is independent — always check it if enabled.
        if enforce_style_present {
            if ctx.ast.css.is_none() {
                let lang_desc = if let Some(ref allowed) = style_langs {
                    pretty_print_langs(allowed)
                } else {
                    "omitted".to_string()
                };
                ctx.diagnostic(
                    format!("The <style> block should be present and its lang attribute should be {}.", lang_desc),
                    Span::new(0, 0),
                );
            }
        }

        // Lang check: only run when style option is explicitly configured.
        if let Some(allowed) = &style_langs {
            if let Some(style) = &ctx.ast.css {
                // Case-insensitive comparison
                let lang = style.lang.as_deref().map(|l| l.to_lowercase());
                let lang_ref = lang.as_deref();
                let allowed_lower: Vec<Option<String>> = allowed.iter()
                    .map(|a| a.as_deref().map(|s| s.to_lowercase()))
                    .collect();

                if !allowed_lower.iter().any(|a| a.as_deref() == lang_ref) {
                    let pretty = pretty_print_langs(allowed);
                    let msg = format!("The lang attribute of the <style> block should be {}.", pretty);

                    let source = &ctx.source[style.span.start as usize..style.span.end as usize];
                    let first_allowed = allowed.iter().find_map(|a| a.as_deref());
                    let replacement = if let Some(target_lang) = first_allowed {
                        if let Some(l) = style.lang.as_deref() {
                            source.replacen(&format!("lang=\"{}\"", l), &format!("lang=\"{}\"", target_lang), 1)
                        } else {
                            source.replacen("<style", &format!("<style lang=\"{}\"", target_lang), 1)
                        }
                    } else {
                        // null-only allowed: remove the lang attribute
                        if let Some(l) = style.lang.as_deref() {
                            let with_space = format!(" lang=\"{}\"", l);
                            if source.contains(&with_space) {
                                source.replacen(&with_space, "", 1)
                            } else {
                                source.replacen(&format!("lang=\"{}\"", l), "", 1)
                            }
                        } else {
                            source.to_string()
                        }
                    };

                    ctx.diagnostic_with_fix(
                        msg,
                        style.span,
                        Fix { span: style.span, replacement },
                    );
                }
            }
        }
        // If style_langs is None (not configured), no lang diagnostics — vendor treats it as a no-op.
    }
}
