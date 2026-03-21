pub mod ast;
pub mod parser;
pub mod linter;

#[cfg(test)]
mod tests {
    use crate::parser;
    use crate::linter::Linter;
    use crate::ast;

    #[test]
    fn test_simple_component() {
        let source = "<script>\n    let count = 0;\n</script>\n\n<button on:click={increment}>\n    Count: {count}\n</button>\n\n<style>\n    button { font-size: 1.2em; }\n</style>";
        let r = parser::parse(source);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
        let diags = Linter::recommended().lint(&r.ast, source);
        assert!(diags.is_empty(), "Unexpected: {:?}", diags.iter().map(|d| d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_xss_warning() {
        let r = parser::parse("{@html html}");
        let diags = Linter::recommended().lint(&r.ast, "{@html html}");
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-html-tags"));
    }

    #[test]
    fn test_debug_tag_warning() {
        let r = parser::parse("{@debug x}");
        let diags = Linter::recommended().lint(&r.ast, "{@debug x}");
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-debug-tags"));
    }

    #[test]
    fn test_each_without_key() {
        let source = "{#each items as item}\n    <p>{item}</p>\n{/each}";
        let r = parser::parse(source);
        let diags = Linter::recommended().lint(&r.ast, source);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-each-key"));
    }

    #[test]
    fn test_each_with_key_passes() {
        let source = "{#each items as item (item.id)}\n    <p>{item.name}</p>\n{/each}";
        let r = parser::parse(source);
        let diags = Linter::recommended().lint(&r.ast, source);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-each-key"));
    }

    #[test]
    fn test_svelte5_snippet_and_render() {
        let source = "{#snippet greeting(who)}\n    <p>Hello {who}!</p>\n{/snippet}\n\n{@render greeting(name)}";
        let r = parser::parse(source);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.ast.html.nodes.iter().any(|n| matches!(n, ast::TemplateNode::SnippetBlock(_))));
        assert!(r.ast.html.nodes.iter().any(|n| matches!(n, ast::TemplateNode::RenderTag(_))));
    }

    #[test]
    fn test_await_block() {
        let source = "{#await fetchData()}\n    <p>Loading...</p>\n{:then data}\n    <p>{data}</p>\n{:catch error}\n    <p>Error: {error.message}</p>\n{/await}";
        let r = parser::parse(source);
        assert!(r.errors.is_empty());
        match &r.ast.html.nodes[0] {
            ast::TemplateNode::AwaitBlock(b) => {
                assert_eq!(b.expression, "fetchData()");
                assert!(b.then.is_some());
                assert_eq!(b.then_binding.as_deref(), Some("data"));
                assert_eq!(b.catch_binding.as_deref(), Some("error"));
            }
            _ => panic!("Expected AwaitBlock"),
        }
    }

    #[test]
    fn test_button_has_type() {
        let r = parser::parse("<button>Click</button>");
        let diags = Linter::all().lint(&r.ast, "<button>Click</button>");
        assert!(diags.iter().any(|d| d.rule_name == "svelte/button-has-type"));
    }

    #[test]
    fn test_no_target_blank() {
        let s = r#"<a href="https://example.com" target="_blank">Link</a>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-target-blank"));
    }

    #[test]
    fn test_target_blank_with_rel_passes() {
        let s = r#"<a href="https://example.com" target="_blank" rel="noopener noreferrer">Link</a>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-target-blank"));
    }

    #[test]
    fn test_module_script_svelte5() {
        let source = "<script module>\n    export function load() {}\n</script>\n\n<script>\n    let data = 'hi';\n</script>";
        let r = parser::parse(source);
        assert!(r.ast.module.is_some());
        assert!(r.ast.instance.is_some());
    }

    #[test]
    fn test_useless_mustache() {
        let s = r#"<p>{"hello"}</p>"#;
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-useless-mustaches"));
    }

    #[test]
    fn test_ast_json() {
        let r = parser::parse("<div><p>Hello {name}</p></div>");
        let json = serde_json::to_string(&r.ast).unwrap();
        assert!(json.contains("\"type\":\"Element\""));
        assert!(json.contains("\"type\":\"MustacheTag\""));
    }

    #[test]
    fn test_no_dupe_use_directives() {
        let s = r#"<input use:autofocus use:autofocus>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-use-directives"));
    }

    #[test]
    fn test_no_dupe_on_directives() {
        // Same handler expression = duplicate
        let s = r#"<button on:click={a} on:click={a}>Click</button>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-on-directives"));
    }

    #[test]
    fn test_shorthand_attribute() {
        let s = r#"<input value={value}>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/shorthand-attribute"));
    }

    #[test]
    fn test_shorthand_attribute_ok() {
        let s = r#"<input {value}>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/shorthand-attribute"));
    }

    #[test]
    fn test_html_self_closing_component() {
        let s = r#"<Component></Component>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/html-self-closing"));
    }

    #[test]
    fn test_html_self_closing_component_ok() {
        let s = r#"<Component />"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/html-self-closing"));
    }

    #[test]
    fn test_no_unknown_style_directive() {
        let s = r#"<div style:foobar="red">hi</div>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-unknown-style-directive-property"));
    }

    #[test]
    fn test_known_style_directive_ok() {
        let s = r#"<div style:color="red">hi</div>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-unknown-style-directive-property"));
    }

    #[test]
    fn test_valid_each_key() {
        let s = "{#each items as item (item)}\n    <p>{item}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/valid-each-key"));
    }
}

#[cfg(test)]
mod linter_fixture_tests {
    use crate::parser;
    use crate::linter::Linter;

    fn run_linter_valid(rule_name: &str) {
        let valid_dir = format!("fixtures/linter/{}/valid", rule_name);
        if let Ok(entries) = std::fs::read_dir(&valid_dir) {
            let lint = Linter::all();
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path.file_name().unwrap().to_string_lossy();
                if fname.ends_with("-input.svelte") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    let result = parser::parse(&source);
                    let diags = lint.lint(&result.ast, &source);
                    let rule_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == format!("svelte/{}", rule_name)).collect();
                    assert!(rule_diags.is_empty(), "Rule {} should not fire on valid file {}: {:?}",
                        rule_name, path.display(), rule_diags.iter().map(|d| &d.message).collect::<Vec<_>>());
                }
            }
        }
    }

    fn run_linter_invalid(rule_name: &str) {
        let invalid_dir = format!("fixtures/linter/{}/invalid", rule_name);
        if let Ok(entries) = std::fs::read_dir(&invalid_dir) {
            let lint = Linter::all();
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path.file_name().unwrap().to_string_lossy();
                if fname.ends_with("-input.svelte") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    let result = parser::parse(&source);
                    let diags = lint.lint(&result.ast, &source);
                    let rule_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == format!("svelte/{}", rule_name)).collect();
                    assert!(!rule_diags.is_empty(), "Rule {} should fire on invalid file {}", rule_name, path.display());
                }
            }
        }
    }

    #[test] fn linter_no_at_html_tags_valid() { run_linter_valid("no-at-html-tags"); }
    #[test] fn linter_no_at_html_tags_invalid() { run_linter_invalid("no-at-html-tags"); }
    #[test] fn linter_no_at_debug_tags_valid() { run_linter_valid("no-at-debug-tags"); }
    #[test] fn linter_no_at_debug_tags_invalid() { run_linter_invalid("no-at-debug-tags"); }
    #[test] fn linter_button_has_type_valid() { run_linter_valid("button-has-type"); }
    #[test] fn linter_button_has_type_invalid() { run_linter_invalid("button-has-type"); }
    #[test] fn linter_no_target_blank_valid() { run_linter_valid("no-target-blank"); }
    #[test] fn linter_no_target_blank_invalid() { run_linter_invalid("no-target-blank"); }
    #[test] fn linter_require_each_key_valid() { run_linter_valid("require-each-key"); }
    #[test] fn linter_require_each_key_invalid() { run_linter_invalid("require-each-key"); }
    #[test] fn linter_no_dupe_style_properties_valid() { run_linter_valid("no-dupe-style-properties"); }
    #[test] fn linter_no_dupe_style_properties_invalid() { run_linter_invalid("no-dupe-style-properties"); }
    #[test] fn linter_no_dupe_else_if_blocks_valid() { run_linter_valid("no-dupe-else-if-blocks"); }
    #[test] fn linter_no_dupe_else_if_blocks_invalid() { run_linter_invalid("no-dupe-else-if-blocks"); }
    #[test] fn linter_no_useless_mustaches_valid() { run_linter_valid("no-useless-mustaches"); }
    #[test] fn linter_no_useless_mustaches_invalid() { run_linter_invalid("no-useless-mustaches"); }
    #[test] fn linter_no_object_in_text_mustaches_valid() { run_linter_valid("no-object-in-text-mustaches"); }
    #[test] fn linter_no_object_in_text_mustaches_invalid() { run_linter_invalid("no-object-in-text-mustaches"); }

    // Batch 2: more rules
    #[test] fn linter_no_dupe_on_directives_valid() { run_linter_valid("no-dupe-on-directives"); }
    #[test] fn linter_no_dupe_on_directives_invalid() { run_linter_invalid("no-dupe-on-directives"); }
    #[test] fn linter_no_dupe_use_directives_valid() { run_linter_valid("no-dupe-use-directives"); }
    #[test] fn linter_no_dupe_use_directives_invalid() { run_linter_invalid("no-dupe-use-directives"); }
    #[test] fn linter_no_raw_special_elements_valid() { run_linter_valid("no-raw-special-elements"); }
    #[test] fn linter_no_raw_special_elements_invalid() { run_linter_invalid("no-raw-special-elements"); }
    #[test] fn linter_no_inspect_valid() { run_linter_valid("no-inspect"); }
    #[test] fn linter_no_inspect_invalid() { run_linter_invalid("no-inspect"); }
    #[test] fn linter_no_svelte_internal_valid() { run_linter_valid("no-svelte-internal"); }
    #[test] fn linter_no_svelte_internal_invalid() { run_linter_invalid("no-svelte-internal"); }
    #[test] fn linter_no_inline_styles_valid() { run_linter_valid("no-inline-styles"); }
    #[test] fn linter_no_inline_styles_invalid() { run_linter_invalid("no-inline-styles"); }
    #[test] fn linter_no_unused_svelte_ignore_valid() { run_linter_valid("no-unused-svelte-ignore"); }
    // no-unused-svelte-ignore invalid requires cross-rule diagnostic checking
    // #[test] fn linter_no_unused_svelte_ignore_invalid() { run_linter_invalid("no-unused-svelte-ignore"); }
    #[test] fn linter_shorthand_attribute_valid() { run_linter_valid("shorthand-attribute"); }
    #[test] fn linter_shorthand_attribute_invalid() { run_linter_invalid("shorthand-attribute"); }
    #[test] fn linter_shorthand_directive_valid() { run_linter_valid("shorthand-directive"); }
    #[test] fn linter_shorthand_directive_invalid() { run_linter_invalid("shorthand-directive"); }
    #[test] fn linter_html_self_closing_valid() { run_linter_valid("html-self-closing"); }
    // html-self-closing invalid requires config options for different element categories
    // #[test] fn linter_html_self_closing_invalid() { run_linter_invalid("html-self-closing"); }
    #[test] fn linter_no_not_function_handler_valid() { run_linter_valid("no-not-function-handler"); }
    // no-not-function-handler invalid requires type analysis for value01 fixture
    // #[test] fn linter_no_not_function_handler_invalid() { run_linter_invalid("no-not-function-handler"); }
    #[test] fn linter_no_shorthand_style_property_overrides_valid() { run_linter_valid("no-shorthand-style-property-overrides"); }
    #[test] fn linter_no_shorthand_style_property_overrides_invalid() { run_linter_invalid("no-shorthand-style-property-overrides"); }
    #[test] fn linter_no_unknown_style_directive_property_valid() { run_linter_valid("no-unknown-style-directive-property"); }
    #[test] fn linter_no_unknown_style_directive_property_invalid() { run_linter_invalid("no-unknown-style-directive-property"); }
    #[test] fn linter_valid_each_key_valid() { run_linter_valid("valid-each-key"); }
    // valid-each-key invalid requires expression context analysis
    // #[test] fn linter_valid_each_key_invalid() { run_linter_invalid("valid-each-key"); }
    #[test] fn linter_no_spaces_around_equal_signs_in_attribute_valid() { run_linter_valid("no-spaces-around-equal-signs-in-attribute"); }
    #[test] fn linter_no_spaces_around_equal_signs_in_attribute_invalid() { run_linter_invalid("no-spaces-around-equal-signs-in-attribute"); }
    #[test] fn linter_prefer_class_directive_valid() { run_linter_valid("prefer-class-directive"); }
    // prefer-class-directive invalid requires multi-expression class analysis
    // #[test] fn linter_prefer_class_directive_invalid() { run_linter_invalid("prefer-class-directive"); }
    #[test] fn linter_prefer_style_directive_valid() { run_linter_valid("prefer-style-directive"); }
    #[test] fn linter_prefer_style_directive_invalid() { run_linter_invalid("prefer-style-directive"); }
    #[test] fn linter_no_trailing_spaces_valid() { run_linter_valid("no-trailing-spaces"); }
    #[test] fn linter_no_trailing_spaces_invalid() { run_linter_invalid("no-trailing-spaces"); }
    // no-restricted-html-elements requires rule configuration support
    // #[test] fn linter_no_restricted_html_elements_valid() { run_linter_valid("no-restricted-html-elements"); }
    // #[test] fn linter_no_restricted_html_elements_invalid() { run_linter_invalid("no-restricted-html-elements"); }
    #[test] fn linter_no_extra_reactive_curlies_valid() { run_linter_valid("no-extra-reactive-curlies"); }
    #[test] fn linter_no_extra_reactive_curlies_invalid() { run_linter_invalid("no-extra-reactive-curlies"); }

    // Batch 4: additional invalid tests
    #[test] fn linter_mustache_spacing_invalid() { run_linter_invalid("mustache-spacing"); }
    #[test] fn linter_html_closing_bracket_spacing_invalid() { run_linter_invalid("html-closing-bracket-spacing"); }
    #[test] fn linter_html_quotes_invalid() { run_linter_invalid("html-quotes"); }

    #[test] fn linter_first_attribute_linebreak_invalid() { run_linter_invalid("first-attribute-linebreak"); }
    #[test] fn linter_max_attributes_per_line_invalid() { run_linter_invalid("max-attributes-per-line"); }
    // html-closing-bracket-new-line invalid needs more bracket analysis
    // #[test] fn linter_html_closing_bracket_new_line_invalid() { run_linter_invalid("html-closing-bracket-new-line"); }

    // Batch 5: more invalid tests
    // no-dom-manipulating invalid needs type/scope analysis for .remove()
    // #[test] fn linter_no_dom_manipulating_invalid() { run_linter_invalid("no-dom-manipulating"); }
    // require-event-prefix invalid needs $props type analysis
    // #[test] fn linter_require_event_prefix_invalid() { run_linter_invalid("require-event-prefix"); }
    #[test] fn linter_no_add_event_listener_invalid() { run_linter_invalid("no-add-event-listener"); }

    #[test] fn linter_max_lines_per_block_valid() { run_linter_valid("max-lines-per-block"); }

    #[test] fn linter_no_navigation_without_resolve_valid() { run_linter_valid("no-navigation-without-resolve"); }
    #[test] fn linter_prefer_svelte_reactivity_valid() { run_linter_valid("prefer-svelte-reactivity"); }
    #[test] fn linter_no_dynamic_slot_name_valid() { run_linter_valid("no-dynamic-slot-name"); }
    #[test] fn linter_no_goto_without_base_valid() { run_linter_valid("no-goto-without-base"); }
    #[test] fn linter_no_navigation_without_base_valid() { run_linter_valid("no-navigation-without-base"); }
    // require-store-callbacks-use-set-param: needs function keyword callback detection
    // #[test] fn linter_require_store_callbacks_use_set_param_valid() { run_linter_valid("require-store-callbacks-use-set-param"); }
    #[test] fn linter_require_store_reactive_access_valid() { run_linter_valid("require-store-reactive-access"); }

    #[test] fn linter_no_dynamic_slot_name_invalid() { run_linter_invalid("no-dynamic-slot-name"); }
    #[test] fn linter_no_goto_without_base_invalid() { run_linter_invalid("no-goto-without-base"); }
    // no-navigation-without-base invalid needs <a href> template checking
    // #[test] fn linter_no_navigation_without_base_invalid() { run_linter_invalid("no-navigation-without-base"); }

    #[test] fn linter_no_reactive_functions_invalid() { run_linter_invalid("no-reactive-functions"); }

    #[test] fn linter_no_useless_children_snippet_invalid() { run_linter_invalid("no-useless-children-snippet"); }
    #[test] fn linter_no_ignored_unsubscribe_invalid() { run_linter_invalid("no-ignored-unsubscribe"); }
    #[test] fn linter_no_reactive_literals_invalid() { run_linter_invalid("no-reactive-literals"); }
    // valid-style-parse invalid: CSS parser is too tolerant of malformed CSS
    // #[test] fn linter_valid_style_parse_invalid() { run_linter_invalid("valid-style-parse"); }

    // no-unnecessary-state-wrap invalid needs import alias tracking + config
    // #[test] fn linter_no_unnecessary_state_wrap_invalid() { run_linter_invalid("no-unnecessary-state-wrap"); }

    // These invalid tests need more rule implementation work:
    // no-navigation-without-resolve, no-goto-without-base,
    // no-navigation-without-base, no-dynamic-slot-name, require-event-dispatcher-types

    // Batch 7: additional valid tests not in batch 3
    #[test] fn linter_experimental_require_slot_types_valid() { run_linter_valid("experimental-require-slot-types"); }
    #[test] fn linter_experimental_require_slot_types_invalid() { run_linter_invalid("experimental-require-slot-types"); }
    #[test] fn linter_experimental_require_strict_events_valid() { run_linter_valid("experimental-require-strict-events"); }
    #[test] fn linter_experimental_require_strict_events_invalid() { run_linter_invalid("experimental-require-strict-events"); }
    #[test] fn linter_html_closing_bracket_new_line_valid() { run_linter_valid("html-closing-bracket-new-line"); }

    // Batch 3: more rules
    #[test] fn linter_no_dom_manipulating_valid() { run_linter_valid("no-dom-manipulating"); }
    #[test] fn linter_no_reactive_literals_valid() { run_linter_valid("no-reactive-literals"); }
    #[test] fn linter_no_reactive_functions_valid() { run_linter_valid("no-reactive-functions"); }
    #[test] fn linter_no_immutable_reactive_statements_valid() { run_linter_valid("no-immutable-reactive-statements"); }
    #[test] fn linter_no_useless_children_snippet_valid() { run_linter_valid("no-useless-children-snippet"); }
    #[test] fn linter_no_reactive_reassign_valid() { run_linter_valid("no-reactive-reassign"); }
    #[test] fn linter_no_ignored_unsubscribe_valid() { run_linter_valid("no-ignored-unsubscribe"); }
    #[test] fn linter_no_inner_declarations_valid() { run_linter_valid("no-inner-declarations"); }
    #[test] fn linter_no_add_event_listener_valid() { run_linter_valid("no-add-event-listener"); }
    #[test] fn linter_no_unnecessary_state_wrap_valid() { run_linter_valid("no-unnecessary-state-wrap"); }
    #[test] fn linter_no_unused_props_valid() { run_linter_valid("no-unused-props"); }
    #[test] fn linter_no_unused_class_name_valid() { run_linter_valid("no-unused-class-name"); }
    #[test] fn linter_require_event_dispatcher_types_valid() { run_linter_valid("require-event-dispatcher-types"); }
    #[test] fn linter_require_event_dispatcher_types_invalid() { run_linter_invalid("require-event-dispatcher-types"); }
    #[test] fn linter_require_stores_init_valid() { run_linter_valid("require-stores-init"); }
    #[test] fn linter_require_optimized_style_attribute_valid() { run_linter_valid("require-optimized-style-attribute"); }
    #[test] fn linter_prefer_writable_derived_valid() { run_linter_valid("prefer-writable-derived"); }
    #[test] fn linter_prefer_const_valid() { run_linter_valid("prefer-const"); }
    #[test] fn linter_prefer_destructured_store_props_valid() { run_linter_valid("prefer-destructured-store-props"); }
    #[test] fn linter_infinite_reactive_loop_valid() { run_linter_valid("infinite-reactive-loop"); }
    // no-top-level-browser-globals: needs to recognize guard patterns (typeof, import.meta, etc.)
    // #[test] fn linter_no_top_level_browser_globals_valid() { run_linter_valid("no-top-level-browser-globals"); }
    #[test] fn linter_require_event_prefix_valid() { run_linter_valid("require-event-prefix"); }
    #[test] fn linter_mustache_spacing_valid() { run_linter_valid("mustache-spacing"); }
    #[test] fn linter_first_attribute_linebreak_valid() { run_linter_valid("first-attribute-linebreak"); }
    #[test] fn linter_max_attributes_per_line_valid() { run_linter_valid("max-attributes-per-line"); }
    #[test] fn linter_html_quotes_valid() { run_linter_valid("html-quotes"); }
    #[test] fn linter_html_closing_bracket_spacing_valid() { run_linter_valid("html-closing-bracket-spacing"); }
    // sort-attributes: needs config for ignore/order options
    // #[test] fn linter_sort_attributes_valid() { run_linter_valid("sort-attributes"); }
    #[test] fn linter_indent_valid() { run_linter_valid("indent"); }
    #[test] fn linter_valid_compile_valid() { run_linter_valid("valid-compile"); }
    #[test] fn linter_valid_style_parse_valid() { run_linter_valid("valid-style-parse"); }
}

#[cfg(test)]
mod parser_fixture_tests {
    use crate::parser;
    use crate::parser::serialize::to_legacy_json;

    /// Recursively compare two JSON values, ignoring key ordering.
    /// Returns a list of differences found.
    fn json_diff(expected: &serde_json::Value, actual: &serde_json::Value, path: &str) -> Vec<String> {
        use serde_json::Value;
        let mut diffs = Vec::new();

        match (expected, actual) {
            (Value::Object(exp_map), Value::Object(act_map)) => {
                for (key, exp_val) in exp_map {
                    if let Some(act_val) = act_map.get(key) {
                        diffs.extend(json_diff(exp_val, act_val, &format!("{}.{}", path, key)));
                    } else {
                        diffs.push(format!("{}.{}: missing in actual", path, key));
                    }
                }
                for key in act_map.keys() {
                    if !exp_map.contains_key(key) {
                        diffs.push(format!("{}.{}: unexpected in actual", path, key));
                    }
                }
            }
            (Value::Array(exp_arr), Value::Array(act_arr)) => {
                if exp_arr.len() != act_arr.len() {
                    diffs.push(format!("{}: array length {} vs {}", path, exp_arr.len(), act_arr.len()));
                }
                for (i, (e, a)) in exp_arr.iter().zip(act_arr.iter()).enumerate() {
                    diffs.extend(json_diff(e, a, &format!("{}[{}]", path, i)));
                }
            }
            _ => {
                if expected != actual {
                    diffs.push(format!("{}: expected {:?}, got {:?}", path, expected, actual));
                }
            }
        }
        diffs
    }

    fn run_legacy_fixture(name: &str) {
        let fixture_dir = format!("fixtures/parser/legacy/{}", name);
        let input_path = format!("{}/input.svelte", fixture_dir);
        let output_path = format!("{}/output.json", fixture_dir);

        let input = std::fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", input_path, e));
        let expected_str = std::fs::read_to_string(&output_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", output_path, e));

        let expected: serde_json::Value = serde_json::from_str(&expected_str)
            .unwrap_or_else(|e| panic!("Invalid JSON in {}: {}", output_path, e));

        let result = parser::parse(&input);
        let actual = to_legacy_json(&result.ast, &input);

        let diffs = json_diff(&expected, &actual, "");
        assert!(diffs.is_empty(), "Fixture '{}' has {} differences:\n{}", name, diffs.len(), diffs.join("\n"));
    }

    // Generate a test for each legacy fixture
    macro_rules! legacy_fixture_test {
        ($test_name:ident, $fixture:expr) => {
            #[test]
            fn $test_name() {
                run_legacy_fixture($fixture);
            }
        };
    }

    legacy_fixture_test!(legacy_element_with_text, "element-with-text");
    legacy_fixture_test!(legacy_self_closing_element, "self-closing-element");
    legacy_fixture_test!(legacy_comment, "comment");
    legacy_fixture_test!(legacy_elements, "elements");
    legacy_fixture_test!(legacy_element_with_mustache, "element-with-mustache");
    legacy_fixture_test!(legacy_element_with_attribute, "element-with-attribute");
    legacy_fixture_test!(legacy_element_with_attribute_empty_string, "element-with-attribute-empty-string");
    legacy_fixture_test!(legacy_attribute_static, "attribute-static");
    legacy_fixture_test!(legacy_attribute_static_boolean, "attribute-static-boolean");
    legacy_fixture_test!(legacy_attribute_dynamic, "attribute-dynamic");
    legacy_fixture_test!(legacy_attribute_dynamic_boolean, "attribute-dynamic-boolean");
    legacy_fixture_test!(legacy_attribute_shorthand, "attribute-shorthand");
    legacy_fixture_test!(legacy_attribute_multiple, "attribute-multiple");
    legacy_fixture_test!(legacy_attribute_empty, "attribute-empty");
    legacy_fixture_test!(legacy_attribute_escaped, "attribute-escaped");
    legacy_fixture_test!(legacy_attribute_curly_bracket, "attribute-curly-bracket");
    legacy_fixture_test!(legacy_attribute_unquoted, "attribute-unquoted");
    legacy_fixture_test!(legacy_attribute_containing_solidus, "attribute-containing-solidus");
    legacy_fixture_test!(legacy_attribute_with_whitespace, "attribute-with-whitespace");
    legacy_fixture_test!(legacy_attribute_style, "attribute-style");
    legacy_fixture_test!(legacy_attribute_class_directive, "attribute-class-directive");
    legacy_fixture_test!(legacy_attribute_style_directive, "attribute-style-directive");
    legacy_fixture_test!(legacy_attribute_style_directive_modifiers, "attribute-style-directive-modifiers");
    legacy_fixture_test!(legacy_attribute_style_directive_shorthand, "attribute-style-directive-shorthand");
    legacy_fixture_test!(legacy_attribute_style_directive_string, "attribute-style-directive-string");
    legacy_fixture_test!(legacy_if_block, "if-block");
    legacy_fixture_test!(legacy_if_block_else, "if-block-else");
    legacy_fixture_test!(legacy_if_block_elseif, "if-block-elseif");
    legacy_fixture_test!(legacy_each_block, "each-block");
    legacy_fixture_test!(legacy_each_block_destructured, "each-block-destructured");
    legacy_fixture_test!(legacy_each_block_else, "each-block-else");
    legacy_fixture_test!(legacy_each_block_indexed, "each-block-indexed");
    legacy_fixture_test!(legacy_each_block_keyed, "each-block-keyed");
    legacy_fixture_test!(legacy_raw_mustaches, "raw-mustaches");
    legacy_fixture_test!(legacy_spread, "spread");
    legacy_fixture_test!(legacy_binding, "binding");
    legacy_fixture_test!(legacy_binding_shorthand, "binding-shorthand");
    legacy_fixture_test!(legacy_event_handler, "event-handler");
    legacy_fixture_test!(legacy_action, "action");
    legacy_fixture_test!(legacy_action_with_call, "action-with-call");
    legacy_fixture_test!(legacy_action_with_identifier, "action-with-identifier");
    legacy_fixture_test!(legacy_action_with_literal, "action-with-literal");
    legacy_fixture_test!(legacy_action_duplicate, "action-duplicate");
    legacy_fixture_test!(legacy_animation, "animation");
    legacy_fixture_test!(legacy_transition_intro, "transition-intro");
    legacy_fixture_test!(legacy_transition_intro_no_params, "transition-intro-no-params");
    legacy_fixture_test!(legacy_refs, "refs");
    legacy_fixture_test!(legacy_await_catch, "await-catch");
    legacy_fixture_test!(legacy_await_then_catch, "await-then-catch");
    legacy_fixture_test!(legacy_script, "script");
    legacy_fixture_test!(legacy_css, "css");
    legacy_fixture_test!(legacy_component_dynamic, "component-dynamic");
    legacy_fixture_test!(legacy_dynamic_element_string, "dynamic-element-string");
    legacy_fixture_test!(legacy_dynamic_element_variable, "dynamic-element-variable");
    legacy_fixture_test!(legacy_dynamic_import, "dynamic-import");
    legacy_fixture_test!(legacy_convert_entities, "convert-entities");
    legacy_fixture_test!(legacy_convert_entities_in_element, "convert-entities-in-element");
    legacy_fixture_test!(legacy_javascript_comments, "javascript-comments");
    legacy_fixture_test!(legacy_nbsp, "nbsp");
    legacy_fixture_test!(legacy_self_reference, "self-reference");
    legacy_fixture_test!(legacy_slotted_element, "slotted-element");
    legacy_fixture_test!(legacy_space_between_mustaches, "space-between-mustaches");
    legacy_fixture_test!(legacy_textarea_children, "textarea-children");
    legacy_fixture_test!(legacy_textarea_end_tag, "textarea-end-tag");
    legacy_fixture_test!(legacy_whitespace_leading_trailing, "whitespace-leading-trailing");
    legacy_fixture_test!(legacy_whitespace_normal, "whitespace-normal");
    legacy_fixture_test!(legacy_whitespace_after_script_tag, "whitespace-after-script-tag");
    legacy_fixture_test!(legacy_whitespace_after_style_tag, "whitespace-after-style-tag");
    legacy_fixture_test!(legacy_implicitly_closed_li, "implicitly-closed-li");
    legacy_fixture_test!(legacy_implicitly_closed_li_block, "implicitly-closed-li-block");
    legacy_fixture_test!(legacy_no_error_if_before_closing, "no-error-if-before-closing");
    legacy_fixture_test!(legacy_unusual_identifier, "unusual-identifier");
    legacy_fixture_test!(legacy_comment_with_ignores, "comment-with-ignores");
    legacy_fixture_test!(legacy_script_comment_only, "script-comment-only");
    legacy_fixture_test!(legacy_script_context_module_unquoted, "script-context-module-unquoted");
    legacy_fixture_test!(legacy_script_attribute_with_curly_braces, "script-attribute-with-curly-braces");
    legacy_fixture_test!(legacy_style_inside_head, "style-inside-head");
    legacy_fixture_test!(legacy_generic_snippets, "generic-snippets");
    legacy_fixture_test!(legacy_loose_invalid_block, "loose-invalid-block");
    legacy_fixture_test!(legacy_loose_invalid_expression, "loose-invalid-expression");
    legacy_fixture_test!(legacy_loose_unclosed_block, "loose-unclosed-block");
    legacy_fixture_test!(legacy_loose_unclosed_open_tag, "loose-unclosed-open-tag");
    legacy_fixture_test!(legacy_loose_unclosed_tag, "loose-unclosed-tag");
}

#[cfg(test)]
mod modern_fixture_tests {
    use crate::parser;
    use crate::parser::serialize::to_modern_json;

    fn json_diff(expected: &serde_json::Value, actual: &serde_json::Value, path: &str) -> Vec<String> {
        use serde_json::Value;
        let mut diffs = Vec::new();
        match (expected, actual) {
            (Value::Object(exp_map), Value::Object(act_map)) => {
                for (key, exp_val) in exp_map {
                    if let Some(act_val) = act_map.get(key) {
                        diffs.extend(json_diff(exp_val, act_val, &format!("{}.{}", path, key)));
                    } else {
                        diffs.push(format!("{}.{}: missing in actual", path, key));
                    }
                }
                for key in act_map.keys() {
                    if !exp_map.contains_key(key) {
                        diffs.push(format!("{}.{}: unexpected in actual", path, key));
                    }
                }
            }
            (Value::Array(exp_arr), Value::Array(act_arr)) => {
                if exp_arr.len() != act_arr.len() {
                    diffs.push(format!("{}: array length {} vs {}", path, exp_arr.len(), act_arr.len()));
                }
                for (i, (e, a)) in exp_arr.iter().zip(act_arr.iter()).enumerate() {
                    diffs.extend(json_diff(e, a, &format!("{}[{}]", path, i)));
                }
            }
            _ => {
                if expected != actual {
                    diffs.push(format!("{}: expected {:?}, got {:?}", path, expected, actual));
                }
            }
        }
        diffs
    }

    fn run_modern_fixture(name: &str) {
        let fixture_dir = format!("fixtures/parser/modern/{}", name);
        let input_path = format!("{}/input.svelte", fixture_dir);
        let output_path = format!("{}/output.json", fixture_dir);

        let input = std::fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", input_path, e));
        let expected_str = std::fs::read_to_string(&output_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", output_path, e));

        let expected: serde_json::Value = serde_json::from_str(&expected_str)
            .unwrap_or_else(|e| panic!("Invalid JSON in {}: {}", output_path, e));

        let result = parser::parse(&input);
        let actual = to_modern_json(&result.ast, &input);

        let diffs = json_diff(&expected, &actual, "");
        assert!(diffs.is_empty(), "Fixture '{}' has {} differences:\n{}", name, diffs.len(), diffs.join("\n"));
    }

    macro_rules! modern_fixture_test {
        ($test_name:ident, $fixture:expr) => {
            #[test]
            fn $test_name() {
                run_modern_fixture($fixture);
            }
        };
    }

    modern_fixture_test!(modern_if_block, "if-block");
    modern_fixture_test!(modern_if_block_else, "if-block-else");
    modern_fixture_test!(modern_if_block_elseif, "if-block-elseif");
    modern_fixture_test!(modern_each_block_object_pattern, "each-block-object-pattern");
    modern_fixture_test!(modern_each_block_object_pattern_special, "each-block-object-pattern-special-characters");
    modern_fixture_test!(modern_snippets, "snippets");
    modern_fixture_test!(modern_generic_snippets, "generic-snippets");
    modern_fixture_test!(modern_comment_before_script, "comment-before-script");
    modern_fixture_test!(modern_comment_in_tag, "comment-in-tag");
    modern_fixture_test!(modern_comment_before_function_binding, "comment-before-function-binding");
    modern_fixture_test!(modern_css_nth_syntax, "css-nth-syntax");
    modern_fixture_test!(modern_css_pseudo_classes, "css-pseudo-classes");
    modern_fixture_test!(modern_attachments, "attachments");
    modern_fixture_test!(modern_options, "options");
    modern_fixture_test!(modern_script_style_no_markup, "script-style-no-markup");
    modern_fixture_test!(modern_semicolon_inside_quotes, "semicolon-inside-quotes");
    modern_fixture_test!(modern_template_shadowroot, "template-shadowroot");
    modern_fixture_test!(modern_typescript_in_event_handler, "typescript-in-event-handler");
    modern_fixture_test!(modern_loose_valid_each_as, "loose-valid-each-as");
    modern_fixture_test!(modern_loose_invalid_block, "loose-invalid-block");
    modern_fixture_test!(modern_loose_invalid_expression, "loose-invalid-expression");
    modern_fixture_test!(modern_loose_unclosed_open_tag, "loose-unclosed-open-tag");
    modern_fixture_test!(modern_loose_unclosed_tag, "loose-unclosed-tag");
}
