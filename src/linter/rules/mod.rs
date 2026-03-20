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
mod no_immutable_reactive_statements;
mod no_dom_manipulating;
mod no_reactive_reassign;
mod no_store_async;
mod prefer_class_directive;
mod no_export_load_in_svelte_module_in_kit_pages;
mod infinite_reactive_loop;
mod no_unnecessary_state_wrap;
mod no_unused_props;
mod prefer_writable_derived;
mod require_stores_init;
mod no_add_event_listener;
mod block_lang;
mod max_lines_per_block;
mod require_optimized_style_attribute;
mod prefer_style_directive;
mod no_spaces_around_equal_signs_in_attribute;
mod no_restricted_html_elements;
mod no_extra_reactive_curlies;

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
        Box::new(no_immutable_reactive_statements::NoImmutableReactiveStatements),
        Box::new(no_dom_manipulating::NoDomManipulating),
        Box::new(no_reactive_reassign::NoReactiveReassign),
        Box::new(no_store_async::NoStoreAsync),
        Box::new(prefer_class_directive::PreferClassDirective),
        Box::new(no_export_load_in_svelte_module_in_kit_pages::NoExportLoadInSvelteModuleInKitPages),
    ]
}

/// Return only recommended (⭐) rules.
pub fn recommended_rules() -> Vec<Box<dyn Rule>> {
    all_rules()
        .into_iter()
        .filter(|r| r.is_recommended())
        .collect()
}
