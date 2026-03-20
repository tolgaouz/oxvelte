//! Lint rule implementations.
//!
//! Each rule is in its own submodule. This module provides:
//! - `all_rules()` — every implemented rule
//! - `recommended_rules()` — only rules marked ⭐

mod no_at_html_tags;
mod no_at_debug_tags;
mod no_dupe_else_if_blocks;
mod no_dupe_style_properties;
mod no_dupe_use_directives;
mod no_dupe_on_directives;
mod require_each_key;
mod no_object_in_text_mustaches;
mod no_useless_mustaches;
mod no_target_blank;
mod button_has_type;
mod no_raw_special_elements;
mod no_inspect;
mod no_svelte_internal;
mod no_inline_styles;
mod valid_each_key;
mod no_not_function_handler;
mod no_ignored_unsubscribe;
mod no_inner_declarations;
mod spaced_html_comment;
mod no_trailing_spaces;
mod require_event_dispatcher_types;
mod no_unused_svelte_ignore;
mod html_self_closing;
mod no_unknown_style_directive_property;
mod no_shorthand_style_property_overrides;
mod shorthand_attribute;
mod shorthand_directive;
mod no_reactive_literals;
mod no_reactive_functions;
mod no_useless_children_snippet;

use super::Rule;

/// Return all implemented lint rules.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(no_at_html_tags::NoAtHtmlTags),
        Box::new(no_at_debug_tags::NoAtDebugTags),
        Box::new(no_dupe_else_if_blocks::NoDupeElseIfBlocks),
        Box::new(no_dupe_style_properties::NoDupeStyleProperties),
        Box::new(no_dupe_use_directives::NoDupeUseDirectives),
        Box::new(no_dupe_on_directives::NoDupeOnDirectives),
        Box::new(require_each_key::RequireEachKey),
        Box::new(no_object_in_text_mustaches::NoObjectInTextMustaches),
        Box::new(no_useless_mustaches::NoUselessMustaches),
        Box::new(no_target_blank::NoTargetBlank),
        Box::new(button_has_type::ButtonHasType),
        Box::new(no_raw_special_elements::NoRawSpecialElements),
        Box::new(no_inspect::NoInspect),
        Box::new(no_svelte_internal::NoSvelteInternal),
        Box::new(no_inline_styles::NoInlineStyles),
        Box::new(valid_each_key::ValidEachKey),
        Box::new(no_not_function_handler::NoNotFunctionHandler),
        Box::new(no_ignored_unsubscribe::NoIgnoredUnsubscribe),
        Box::new(no_inner_declarations::NoInnerDeclarations),
        Box::new(spaced_html_comment::SpacedHtmlComment),
        Box::new(no_trailing_spaces::NoTrailingSpaces),
        Box::new(require_event_dispatcher_types::RequireEventDispatcherTypes),
        Box::new(no_unused_svelte_ignore::NoUnusedSvelteIgnore),
        Box::new(html_self_closing::HtmlSelfClosing),
        Box::new(no_unknown_style_directive_property::NoUnknownStyleDirectiveProperty),
        Box::new(no_shorthand_style_property_overrides::NoShorthandStylePropertyOverrides),
        Box::new(shorthand_attribute::ShorthandAttribute),
        Box::new(shorthand_directive::ShorthandDirective),
        Box::new(no_reactive_literals::NoReactiveLiterals),
        Box::new(no_reactive_functions::NoReactiveFunctions),
        Box::new(no_useless_children_snippet::NoUselessChildrenSnippet),
    ]
}

/// Return only recommended (⭐) rules.
pub fn recommended_rules() -> Vec<Box<dyn Rule>> {
    all_rules()
        .into_iter()
        .filter(|r| r.is_recommended())
        .collect()
}
