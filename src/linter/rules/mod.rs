//! Lint rule implementations.
//!
//! Each rule is in its own submodule. This module provides:
//! - `all_rules()` — every implemented rule
//! - `recommended_rules()` — only rules marked ⭐

mod no_at_html_tags;
mod no_at_debug_tags;
mod no_dupe_else_if_blocks;
mod no_dupe_style_properties;
mod require_each_key;
mod no_object_in_text_mustaches;
mod no_useless_mustaches;
mod no_target_blank;
mod button_has_type;
mod no_raw_special_elements;

use super::Rule;

/// Return all implemented lint rules.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(no_at_html_tags::NoAtHtmlTags),
        Box::new(no_at_debug_tags::NoAtDebugTags),
        Box::new(no_dupe_else_if_blocks::NoDupeElseIfBlocks),
        Box::new(no_dupe_style_properties::NoDupeStyleProperties),
        Box::new(require_each_key::RequireEachKey),
        Box::new(no_object_in_text_mustaches::NoObjectInTextMustaches),
        Box::new(no_useless_mustaches::NoUselessMustaches),
        Box::new(no_target_blank::NoTargetBlank),
        Box::new(button_has_type::ButtonHasType),
        Box::new(no_raw_special_elements::NoRawSpecialElements),
    ]
}

/// Return only recommended (⭐) rules.
pub fn recommended_rules() -> Vec<Box<dyn Rule>> {
    all_rules()
        .into_iter()
        .filter(|r| r.is_recommended())
        .collect()
}
