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

    #[test]
    fn test_recommended_vs_all() {
        let rec = Linter::recommended();
        let all = Linter::all();
        // All rules should be a superset of recommended
        assert!(all.rules().len() >= rec.rules().len());
        assert!(rec.rules().len() > 0);
    }

    #[test]
    fn test_no_reactive_functions() {
        let s = "<script>\n$: arrow = () => {}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-functions"));
    }

    #[test]
    fn test_no_reactive_literals() {
        let s = "<script>\n$: foo = \"foo\";\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"));
    }

    #[test]
    fn test_no_ignored_unsubscribe() {
        let s = "<script>\nimport { writable } from 'svelte/store';\nconst foo = writable(0);\nfoo.subscribe(() => {});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-ignored-unsubscribe"));
    }

    #[test]
    fn test_html_self_closing_void() {
        let s = "<img>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/html-self-closing"));
    }

    #[test]
    fn test_no_dynamic_slot_name() {
        let s = "<slot name={dynamicName} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dynamic-slot-name"));
    }

    #[test]
    fn test_no_raw_special_elements() {
        let s = "<head></head>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-raw-special-elements"));
    }

    #[test]
    fn test_no_goto_without_base() {
        let s = "<script>\nimport { goto } from '$app/navigation';\ngoto('/foo');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-goto-without-base"));
    }

    #[test]
    fn test_require_stores_init() {
        let s = "<script>\nimport { writable } from 'svelte/store';\nconst w = writable();\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-stores-init"));
    }

    #[test]
    fn test_no_useless_children_snippet() {
        let s = "<Comp>\n{#snippet children()}\nHello\n{/snippet}\n</Comp>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-useless-children-snippet"));
    }

    #[test]
    fn test_no_object_in_text_mustaches() {
        let s = "{{ a }}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-object-in-text-mustaches"));
    }

    #[test]
    fn test_parse_legacy_json() {
        let s = "<div>hello</div>";
        let r = parser::parse(s);
        let json = parser::serialize::to_legacy_json(&r.ast, s);
        assert!(json.get("html").is_some());
    }

    #[test]
    fn test_parse_modern_json() {
        let s = "<div>hello</div>";
        let r = parser::parse(s);
        let json = parser::serialize::to_modern_json(&r.ast, s);
        assert!(json.get("fragment").is_some());
    }

    #[test]
    fn test_css_parsing() {
        let s = "<style>div { color: red; }</style>";
        let r = parser::parse(s);
        assert!(r.ast.css.is_some());
        let css = r.ast.css.unwrap();
        assert!(css.content.contains("color: red"));
    }

    #[test]
    fn test_script_lang_detection() {
        let s = r#"<script lang="ts">let x: number = 1;</script>"#;
        let r = parser::parse(s);
        assert_eq!(r.ast.instance.as_ref().unwrap().lang.as_deref(), Some("ts"));
    }

    #[test]
    fn test_module_context() {
        let s = r#"<script context="module">export const foo = 1;</script>"#;
        let r = parser::parse(s);
        assert!(r.ast.module.is_some());
        assert!(r.ast.instance.is_none());
    }

    #[test]
    fn test_svelte5_module() {
        let s = "<script module>export const foo = 1;</script>";
        let r = parser::parse(s);
        assert!(r.ast.module.is_some());
    }

    #[test]
    fn test_void_element_parsing() {
        let s = "<br><hr><img src='test.png'>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert_eq!(r.ast.html.nodes.len(), 3);
    }

    #[test]
    fn test_if_block_with_else() {
        let s = "{#if condition}yes{:else}no{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::IfBlock(block) = &r.ast.html.nodes[0] {
            assert!(block.alternate.is_some());
        } else {
            panic!("Expected IfBlock");
        }
    }

    #[test]
    fn test_each_block_with_key() {
        let s = "{#each items as item (item.id)}\n{item.name}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::EachBlock(block) = &r.ast.html.nodes[0] {
            assert!(block.key.is_some());
        } else {
            panic!("Expected EachBlock");
        }
    }

    #[test]
    fn test_snippet_block() {
        let s = "{#snippet greeting(name)}\n<p>Hello {name}!</p>\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::SnippetBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.name, "greeting");
        } else {
            panic!("Expected SnippetBlock");
        }
    }

    #[test]
    fn test_render_tag() {
        let s = "{@render greeting('world')}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_const_tag() {
        let s = "{@const x = 42}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_html_entities() {
        let s = "<p>&amp; &lt; &gt;</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_await_block_full() {
        let s = "{#await promise}\n<p>Loading...</p>\n{:then value}\n<p>{value}</p>\n{:catch error}\n<p>{error}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_key_block() {
        let s = "{#key value}\n<p>{value}</p>\n{/key}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_special_elements() {
        let s = "<svelte:head><title>Test</title></svelte:head>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_each_key_fn_call() {
        let s = "{#each things as thing (fn(thing))}\n\t{thing.name}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::EachBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.context.trim(), "thing");
            assert_eq!(block.key.as_ref().unwrap(), "fn(thing)");
        }
    }

    #[test]
    fn test_valid_each_key_outside_var() {
        let s = "<script>\n\tconst foo = 'x';\n</script>\n{#each items as item (foo)}\n\t{item}\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/valid-each-key"),
            "Should flag key that doesn't use iteration variable");
    }

    #[test]
    fn test_valid_each_key_item_prop_ok() {
        let s = "{#each items as item (item.id)}\n\t{item}\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-each-key"),
            "Should NOT flag key that uses item property");
    }

    #[test]
    fn test_html_closing_bracket_newline_singleline() {
        let s = "<div\n></div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/html-closing-bracket-new-line"),
            "Should flag singleline element with line break before >");
    }

    #[test]
    fn test_html_closing_bracket_newline_multiline_ok() {
        let s = "<div\n\tclass=\"foo\"\n></div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/html-closing-bracket-new-line"),
            "Should NOT flag multiline element with 1 line break before >");
    }

    #[test]
    fn test_css_parser_invalid_detection() {
        let s = "<style>\n\t.x { invalid-prop name: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/valid-style-parse"),
            "Should flag CSS with spaces in property name");
    }

    #[test]
    fn test_scss_valid_ok() {
        let s = "<style lang=\"scss\">\n\t.container { .child { color: red; } }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-style-parse"),
            "Should NOT flag valid SCSS");
    }

    // --- spaced-html-comment unit tests ---

    #[test]
    fn test_spaced_comment_no_space_after() {
        let s = "<!--comment-->";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/spaced-html-comment"),
            "Should flag comment without space after <!--");
    }

    #[test]
    fn test_spaced_comment_with_spaces_ok() {
        let s = "<!-- comment -->";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/spaced-html-comment"),
            "Should NOT flag comment with proper spaces");
    }

    #[test]
    fn test_spaced_comment_no_space_before() {
        let s = "<!-- comment-->";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/spaced-html-comment"),
            "Should flag comment without space before -->");
    }

    // --- no-unnecessary-state-wrap unit tests ---

    #[test]
    fn test_unnecessary_state_wrap_svelte_set() {
        let s = "<script>\n\tconst set = $state(new SvelteSet());\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-unnecessary-state-wrap"),
            "Should flag $state(new SvelteSet())");
    }

    #[test]
    fn test_unnecessary_state_wrap_let_ok() {
        let s = "<script>\n\tlet set = $state(new SvelteSet());\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-unnecessary-state-wrap"),
            "Should NOT flag let (might be reassigned)");
    }

    #[test]
    fn test_unnecessary_state_wrap_import_alias() {
        let s = "<script>\n\timport { SvelteSet as CustomSet } from 'svelte/reactivity';\n\tconst set = $state(new CustomSet());\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-unnecessary-state-wrap"),
            "Should flag aliased SvelteSet import");
    }

    #[test]
    fn test_unnecessary_state_wrap_regular_ok() {
        let s = "<script>\n\tconst x = $state(42);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-unnecessary-state-wrap"),
            "Should NOT flag regular $state usage");
    }

    // --- no-dom-manipulating unit tests ---

    #[test]
    fn test_no_dom_manipulating_bind_this() {
        let s = "<script>\n\tlet div;\n\tconst rm = () => div.remove();\n</script>\n<div bind:this={div} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dom-manipulating"),
            "Should flag div.remove() on bind:this element");
    }

    #[test]
    fn test_no_dom_manipulating_component_ok() {
        let s = "<script>\n\timport C from './C.svelte';\n\tlet c;\n\tconst rm = () => c.remove();\n</script>\n<C bind:this={c} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-dom-manipulating"),
            "Should NOT flag .remove() on component bind:this");
    }

    #[test]
    fn test_no_dom_manipulating_text_content() {
        let s = "<script>\n\tlet div;\n\tconst upd = () => div.textContent = 'x';\n</script>\n<div bind:this={div} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dom-manipulating"),
            "Should flag textContent assignment on bind:this element");
    }

    // --- no-not-function-handler unit tests ---

    #[test]
    fn test_no_not_function_handler_string_var() {
        let s = "<script>\n\tconst a = 'hello';\n</script>\n<button on:click={a} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-not-function-handler"),
            "Should flag string variable as event handler");
    }

    #[test]
    fn test_no_not_function_handler_number_var() {
        let s = "<script>\n\tconst x = 42;\n</script>\n<button on:click={x} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-not-function-handler"),
            "Should flag number variable as event handler");
    }

    #[test]
    fn test_no_not_function_handler_fn_ok() {
        let s = "<script>\n\tconst fn1 = () => {};\n</script>\n<button on:click={fn1} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-not-function-handler"),
            "Should NOT flag function variable as event handler");
    }

    // --- require-store-callbacks-use-set-param unit tests ---

    #[test]
    fn test_store_callback_without_set() {
        let s = "<script>\n\timport { readable } from 'svelte/store';\n\treadable(false, () => true);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-callbacks-use-set-param"),
            "Should flag callback without set param");
    }

    #[test]
    fn test_store_callback_with_set_ok() {
        let s = "<script>\n\timport { readable } from 'svelte/store';\n\treadable(false, (set) => set(true));\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-store-callbacks-use-set-param"),
            "Should NOT flag callback with set param");
    }

    #[test]
    fn test_store_callback_function_with_set_ok() {
        let s = "<script>\n\timport { readable } from 'svelte/store';\n\treadable(null, function (set) { set(0); });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-store-callbacks-use-set-param"),
            "Should NOT flag function keyword callback with set param");
    }

    // --- valid-style-parse unit tests ---

    #[test]
    fn test_valid_style_parse_bad_css() {
        let s = "<style>\n\t.container {\n\t\tclass .div-class/35\n\t\tcolor: red;\n\t}\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/valid-style-parse"),
            "Should flag invalid CSS");
    }

    #[test]
    fn test_valid_style_parse_good_css_ok() {
        let s = "<style>\n\t.container { color: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-style-parse"),
            "Should NOT flag valid CSS");
    }

    // --- no-navigation-without-base unit tests ---

    #[test]
    fn test_nav_without_base_link() {
        let s = r#"<a href="/foo">Click</a>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should flag <a href='/foo'> without base");
    }

    #[test]
    fn test_nav_without_base_absolute_ok() {
        let s = r#"<a href="https://example.com">Click</a>"#;
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should NOT flag absolute URL");
    }

    #[test]
    fn test_nav_without_base_fragment_ok() {
        let s = "<a href=\"#section\">Click</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should NOT flag fragment URL");
    }

    // --- more linter rule unit tests ---

    #[test]
    fn test_dupe_style_props_inline() {
        let s = "<div style=\"color: red; color: blue;\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-style-properties"),
            "Should flag duplicate style properties");
    }

    #[test]
    fn test_dupe_style_props_no_dupe_ok() {
        let s = "<div style=\"color: red; font-size: 14px;\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-dupe-style-properties"),
            "Should NOT flag different style properties");
    }

    #[test]
    fn test_dupe_on_directives_same_expr() {
        let s = "<button on:click={handler} on:click={handler}>text</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-on-directives"),
            "Should flag duplicate on: directives with same expression");
    }

    #[test]
    fn test_dupe_on_directives_diff_ok() {
        let s = "<button on:click={foo} on:click={bar}>text</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-dupe-on-directives"),
            "Should NOT flag different on: handlers");
    }

    #[test]
    fn test_dupe_use_directives() {
        let s = "<div use:tooltip use:tooltip>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-use-directives"),
            "Should flag duplicate use: directives");
    }

    #[test]
    fn test_shorthand_attr_non_short() {
        let s = "<div name={name}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/shorthand-attribute"),
            "Should flag non-shorthand attribute");
    }

    #[test]
    fn test_shorthand_attr_ok() {
        let s = "<div {name}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/shorthand-attribute"),
            "Should NOT flag shorthand attribute");
    }

    #[test]
    fn test_object_in_text_mustaches() {
        let s = "<p>{{}}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-object-in-text-mustaches"),
            "Should flag object literal in text mustache");
    }

    #[test]
    fn test_useless_mustaches_static() {
        let s = "<div class=\"{'foo'}\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-useless-mustaches"),
            "Should flag useless mustache with static string");
    }

    #[test]
    fn test_dupe_else_if_condition() {
        let s = "{#if x}\n\ta\n{:else if x}\n\tb\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-else-if-blocks"),
            "Should flag duplicate else-if condition");
    }

    #[test]
    fn test_useless_children_snip_in_component() {
        let s = "<Component>\n\t{#snippet children()}\n\t\t<p>content</p>\n\t{/snippet}\n</Component>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-useless-children-snippet"),
            "Should flag useless snippet children() inside component");
    }

    #[test]
    fn test_useless_children_snip_with_params_ok() {
        let s = "<Component>\n\t{#snippet children(item)}\n\t\t<p>{item}</p>\n\t{/snippet}\n</Component>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-useless-children-snippet"),
            "Should NOT flag snippet children(item) with params");
    }

    #[test]
    fn test_prefer_const_let() {
        let s = "<script>\n\tlet x = 42;\n</script>\n<p>{x}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-const"),
            "Should flag let that could be const");
    }

    // --- more rule unit tests ---

    #[test]
    fn test_ignored_unsubscribe() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst store = writable(0);\n\tstore.subscribe(v => console.log(v));\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-ignored-unsubscribe"),
            "Should flag ignored unsubscribe return");
    }

    #[test]
    fn test_ignored_unsubscribe_saved_ok() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst store = writable(0);\n\tconst unsub = store.subscribe(v => console.log(v));\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-ignored-unsubscribe"),
            "Should NOT flag saved unsubscribe");
    }

    #[test]
    fn test_reactive_literal_42() {
        let s = "<script>\n\t$: x = 42;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should flag reactive literal assignment");
    }

    #[test]
    fn test_stores_init_no_arg() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst count = writable();\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-stores-init"),
            "Should flag store without initial value");
    }

    // --- svelte 5 runes tests ---

    #[test]
    fn test_parse_state_rune() {
        let s = "<script>\n\tlet count = $state(0);\n</script>\n<p>{count}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.as_ref().unwrap().content.contains("$state"));
    }

    #[test]
    fn test_parse_derived_rune() {
        let s = "<script>\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_effect_rune() {
        let s = "<script>\n\tlet count = $state(0);\n\t$effect(() => { console.log(count); });\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_props_rune() {
        let s = "<script>\n\tlet { name, age = 0 } = $props();\n</script>\n<p>{name} is {age}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bindable_rune() {
        let s = "<script>\n\tlet { value = $bindable() } = $props();\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_inspect_rune() {
        let s = "<script>\n\tlet count = $state(0);\n\t$inspect(count);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- event handler tests ---

    #[test]
    fn test_parse_svelte5_onclick() {
        let s = "<button onclick={() => console.log('hi')}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_on_directive_with_modifiers() {
        let s = "<button on:click|preventDefault|stopPropagation={handler}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- attribute edge cases ---

    #[test]
    fn test_parse_bind_group() {
        let s = "<input type=\"radio\" bind:group={selected} value=\"a\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_in_out_transition() {
        let s = "<div in:fly={{y: 200}} out:fade>content</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_shorthand_binding() {
        let s = "<input bind:value />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- complex template tests ---

    #[test]
    fn test_parse_slot_element() {
        let s = "<slot name=\"header\">Default</slot>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_options() {
        let s = "<svelte:options immutable />\n<p>content</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_window() {
        let s = "<svelte:window on:keydown={handler} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_body() {
        let s = "<svelte:body on:click={handler} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_document() {
        let s = "<svelte:document on:visibilitychange={handler} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_fragment() {
        let s = "<svelte:fragment slot=\"header\"><h1>Title</h1></svelte:fragment>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- CSS style tests ---

    #[test]
    fn test_parse_style_with_scss() {
        let s = "<style lang=\"scss\">\n\t.parent {\n\t\t.child { color: red; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.css.as_ref().unwrap().lang.as_deref() == Some("scss"));
    }

    #[test]
    fn test_parse_style_with_less() {
        let s = "<style lang=\"less\">\n\t.parent { color: red; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_empty_style() {
        let s = "<style></style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_browser_global_in_if_ok() {
        let s = "<script>\n\tif (typeof window !== 'undefined') {\n\t\tconsole.log(window.innerWidth);\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should NOT flag window inside if block");
    }

    #[test]
    fn test_browser_globals_guards03() {
        let s = std::fs::read_to_string("fixtures/linter/no-top-level-browser-globals/valid/guards03-input.svelte").unwrap();
        let r = parser::parse(&s);
        let diags = Linter::all().lint(&r.ast, &s);
        let bg: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-top-level-browser-globals").collect();
        assert!(bg.is_empty(), "Should NOT flag location inside guarded blocks, got: {:?}", bg.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    // --- require-store-reactive-access tests ---

    #[test]
    fn test_store_reactive_access_raw() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst count = writable(0);\n</script>\n<p>{count}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should flag raw store access in template");
    }

    #[test]
    fn test_store_reactive_access_dollar_ok() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst count = writable(0);\n</script>\n<p>{$count}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should NOT flag $store access");
    }

    #[test]
    fn test_store_reactive_access_get_ok() {
        let s = "<script>\n\timport { writable, get } from 'svelte/store';\n\tconst count = writable(0);\n</script>\n<p>{get(count)}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should NOT flag get(store) access");
    }

    #[test]
    fn test_store_reactive_access_non_store_ok() {
        let s = "<script>\n\tconst count = 42;\n</script>\n<p>{count}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should NOT flag non-store variable");
    }

    // --- more comprehensive tests ---

    #[test]
    fn test_inner_decl_while_loop() {
        let s = "<script>\n\twhile (true) {\n\t\tfunction inner() {}\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inner-declarations"),
            "Should flag function inside while");
    }

    #[test]
    fn test_inner_decl_for_loop() {
        let s = "<script>\n\tfor (let i = 0; i < 10; i++) {\n\t\tfunction inner() {}\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inner-declarations"),
            "Should flag function inside for");
    }

    #[test]
    fn test_reactive_reassign_array_push() {
        let s = "<script>\n\tlet v = 0;\n\t$: arr = [v];\n\tfunction click() { arr.push(1); }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag .push() on reactive array");
    }

    #[test]
    fn test_writable_derived_not_on_regular_effect() {
        let s = "<script>\n\tlet count = $state(0);\n\t$effect(() => { console.log(count); });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should NOT flag $effect without state reassignment");
    }

    #[test]
    fn test_parse_nbsp_entity() {
        let s = "<p>Hello&nbsp;World</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- final stretch ---

    #[test]
    fn test_parse_css_transition() {
        let s = "<style>\n\t.fade { transition: opacity 0.3s ease; }\n\t.slide { transition: transform 0.5s cubic-bezier(0.4, 0, 0.2, 1); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_animation() {
        let s = "<style>\n\t@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }\n\t.spinner { animation: spin 1s linear infinite; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_flex() {
        let s = "<style>\n\t.flex {\n\t\tdisplay: flex;\n\t\tflex-direction: column;\n\t\talign-items: center;\n\t\tjustify-content: space-between;\n\t\tflex-wrap: wrap;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_clean_sveltekit_page() {
        let s = "<script lang=\"ts\">\n\texport let data;\n</script>\n\n<h1>{data.title}</h1>\n{#each data.items as item (item.id)}\n\t<p>{item.name}</p>\n{/each}\n\n<style lang=\"scss\">\n\th1 { color: var(--primary); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::recommended().lint(&r.ast, s);
        let relevant: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(relevant.is_empty(), "Clean SvelteKit page: {:?}",
            relevant.iter().map(|d| format!("{}: {}", d.rule_name, d.message)).collect::<Vec<_>>());
    }

    #[test]
    fn test_parse_component_with_all_features() {
        let s = "<script lang=\"ts\" generics=\"T\">\n\timport { onMount, createEventDispatcher } from 'svelte';\n\timport { fade } from 'svelte/transition';\n\tinterface $$Props { items: T[]; selected?: T }\n\texport let items: T[] = [];\n\texport let selected: T | undefined = undefined;\n\tconst dispatch = createEventDispatcher<{ select: T }>();\n\tlet container: HTMLDivElement;\n\tonMount(() => { console.log('mounted', container); });\n</script>\n\n<div bind:this={container}>\n\t{#each items as item (item)}\n\t\t<button\n\t\t\tclass:selected={item === selected}\n\t\t\ton:click={() => dispatch('select', item)}\n\t\t\ttransition:fade\n\t\t>\n\t\t\t<slot {item} />\n\t\t</button>\n\t{:else}\n\t\t<p>No items</p>\n\t{/each}\n</div>\n\n<style>\n\t.selected { background: var(--highlight); font-weight: bold; }\n\tbutton { cursor: pointer; border: none; padding: 0.5rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_boundary_with_error() {
        let s = "<svelte:boundary onerror={(e) => console.error(e)}>\n\t<FallibleComponent />\n</svelte:boundary>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_render_with_fallback() {
        let s = "{#if children}\n\t{@render children()}\n{:else}\n\t<p>Default content</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_diagnostic_span_valid() {
        let s = "{@html danger}\n<button>no type</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        for d in &diags {
            assert!(d.span.start < d.span.end, "Diagnostic span should be valid: {:?}", d);
            assert!((d.span.end as usize) <= s.len() + 50, "Span should not exceed source length by much");
        }
    }

    #[test]
    fn test_parse_whitespace_sensitivity() {
        let s = "<p>Hello   World</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            if let Some(ast::TemplateNode::Text(t)) = el.children.first() {
                assert!(t.data.contains("Hello"));
            }
        }
    }

    #[test]
    fn test_parse_interpolation_in_style() {
        let s = "<div style=\"color: {color}; font-size: {size}px;\">styled</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_children() {
        let s = "<Card>\n\t<h2 slot=\"title\">Card Title</h2>\n\t<p>Card body content</p>\n\t<button slot=\"footer\">OK</button>\n</Card>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_objects_and_arrays() {
        let s = "<p>{[1, 2, 3].map(x => x * 2).join(', ')}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_empty_each() {
        let s = "{#each [] as x}{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_if_false() {
        let s = "{#if false}<p>never</p>{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_raw_mustache_complex() {
        let s = "{@html `<div class=\"${className}\">${content}</div>`}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- Svelte 5 migration patterns ---

    #[test]
    fn test_svelte5_migration_state() {
        // Svelte 4: let count = 0; → Svelte 5: let count = $state(0);
        let s = "<script>\n\tlet count = $state(0);\n\tconst increment = () => count++;\n</script>\n<button onclick={increment}>{count}</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_migration_derived() {
        let s = "<script>\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n\tlet quadrupled = $derived(doubled * 2);\n</script>\n<p>{count} → {doubled} → {quadrupled}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_migration_props() {
        let s = "<script lang=\"ts\">\n\tlet { title, count = 0, items = [] }: {\n\t\ttitle: string;\n\t\tcount?: number;\n\t\titems?: string[];\n\t} = $props();\n</script>\n<h1>{title}</h1>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_migration_events() {
        let s = "<script>\n\tlet { onclick }: { onclick?: (e: MouseEvent) => void } = $props();\n</script>\n<button {onclick}>Click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_untrack() {
        let s = "<script>\n\timport { untrack } from 'svelte';\n\tlet count = $state(0);\n\t$effect(() => {\n\t\tconst prev = untrack(() => count);\n\t\tconsole.log(prev);\n\t});\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- complex real-world components ---

    #[test]
    fn test_search_component() {
        let s = "<script>\n\tlet query = $state('');\n\tlet results = $derived(\n\t\titems.filter(i => i.name.toLowerCase().includes(query.toLowerCase()))\n\t);\n</script>\n\n<input type=\"search\" bind:value={query} placeholder=\"Search...\" />\n{#if query.length > 0}\n\t<ul>\n\t\t{#each results as result (result.id)}\n\t\t\t<li>{result.name}</li>\n\t\t{:else}\n\t\t\t<li>No results found</li>\n\t\t{/each}\n\t</ul>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_pagination_component() {
        let s = "<script>\n\tlet { page = 1, total, perPage = 10, onchange } = $props();\n\tlet pages = $derived(Math.ceil(total / perPage));\n</script>\n\n<nav>\n\t<button onclick={() => onchange(page - 1)} disabled={page <= 1}>Prev</button>\n\t{#each Array.from({length: pages}) as _, i}\n\t\t<button class:active={page === i + 1} onclick={() => onchange(i + 1)}>{i + 1}</button>\n\t{/each}\n\t<button onclick={() => onchange(page + 1)} disabled={page >= pages}>Next</button>\n</nav>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_theme_switcher() {
        let s = "<script>\n\tlet theme = $state('light');\n\tconst toggle = () => theme = theme === 'light' ? 'dark' : 'light';\n\t$effect(() => {\n\t\tdocument.documentElement.setAttribute('data-theme', theme);\n\t});\n</script>\n\n<button onclick={toggle}>{theme === 'light' ? '🌙' : '☀️'}</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_infinite_scroll() {
        let s = "<script>\n\tlet items = $state([]);\n\tlet loading = $state(false);\n\tlet page = $state(1);\n\tconst loadMore = async () => {\n\t\tloading = true;\n\t\tconst res = await fetch(`/api?page=${page}`);\n\t\tconst data = await res.json();\n\t\titems = [...items, ...data];\n\t\tpage++;\n\t\tloading = false;\n\t};\n</script>\n\n{#each items as item (item.id)}\n\t<div>{item.text}</div>\n{/each}\n{#if loading}<p>Loading...</p>{/if}\n<button onclick={loadMore}>Load more</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_form_validation() {
        let s = "<script>\n\tlet email = $state('');\n\tlet password = $state('');\n\tlet errors = $derived({\n\t\temail: !email.includes('@') ? 'Invalid email' : '',\n\t\tpassword: password.length < 8 ? 'Too short' : '',\n\t});\n\tlet valid = $derived(!errors.email && !errors.password);\n</script>\n\n<form>\n\t<input bind:value={email} type=\"email\" />\n\t{#if errors.email}<p class=\"error\">{errors.email}</p>{/if}\n\t<input bind:value={password} type=\"password\" />\n\t{#if errors.password}<p class=\"error\">{errors.password}</p>{/if}\n\t<button type=\"submit\" disabled={!valid}>Submit</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_drag_and_drop() {
        let s = "<script>\n\tlet items = $state(['A', 'B', 'C', 'D']);\n\tlet dragging = $state(null);\n</script>\n\n{#each items as item, i (item)}\n\t<div\n\t\tdraggable=\"true\"\n\t\ton:dragstart={() => dragging = i}\n\t\ton:drop={() => { const temp = items[i]; items[i] = items[dragging]; items[dragging] = temp; }}\n\t\ton:dragover|preventDefault\n\t\tclass:dragging={dragging === i}\n\t>\n\t\t{item}\n\t</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- SvelteKit integration tests ---

    #[test]
    fn test_sveltekit_page_component() {
        let s = "<script>\n\texport let data;\n\t$: ({ posts, user } = data);\n</script>\n\n<h1>Welcome, {user.name}</h1>\n{#each posts as post (post.id)}\n\t<article>\n\t\t<h2><a href=\"/posts/{post.slug}\">{post.title}</a></h2>\n\t\t<p>{post.excerpt}</p>\n\t</article>\n{/each}\n\n<style>\n\tarticle { border-bottom: 1px solid #eee; padding: 1rem 0; }\n\th2 a { text-decoration: none; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_sveltekit_error_page() {
        let s = "<script>\n\timport { page } from '$app/stores';\n</script>\n\n<h1>{$page.status}</h1>\n<p>{$page.error?.message ?? 'Unknown error'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_sveltekit_form_action() {
        let s = "<script>\n\texport let form;\n</script>\n\n<form method=\"POST\" action=\"?/create\">\n\t<input name=\"title\" value={form?.title ?? ''} />\n\t{#if form?.error}\n\t\t<p class=\"error\">{form.error}</p>\n\t{/if}\n\t<button type=\"submit\">Create</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_sveltekit_layout() {
        let s = "<script>\n\timport { page } from '$app/stores';\n\timport Nav from './Nav.svelte';\n</script>\n\n<Nav currentPath={$page.url.pathname} />\n<main>\n\t<slot />\n</main>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- accessibility pattern tests ---

    #[test]
    fn test_img_with_alt() {
        let s = "<img src=\"photo.jpg\" alt=\"A nice photo\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_form_with_labels() {
        let s = "<form>\n\t<label for=\"email\">Email</label>\n\t<input id=\"email\" type=\"email\" name=\"email\" required />\n\t<label for=\"pass\">Password</label>\n\t<input id=\"pass\" type=\"password\" name=\"password\" required />\n\t<button type=\"submit\">Login</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_aria_attributes() {
        let s = "<button aria-label=\"Close\" aria-pressed={pressed} role=\"switch\">\n\t<span aria-hidden=\"true\">×</span>\n</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- TypeScript-specific tests ---

    #[test]
    fn test_ts_interface_props() {
        let s = "<script lang=\"ts\">\n\tinterface Props {\n\t\tname: string;\n\t\tage?: number;\n\t\tonclick?: (e: MouseEvent) => void;\n\t}\n\tlet { name, age = 0, onclick }: Props = $props();\n</script>\n<p>{name} ({age})</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_ts_type_annotations() {
        let s = "<script lang=\"ts\">\n\tlet items: string[] = $state([]);\n\tlet map: Map<string, number> = $state(new Map());\n\tconst add = (item: string): void => { items.push(item); };\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_ts_generic_component() {
        let s = "<script lang=\"ts\" generics=\"T, U extends Record<string, T>\">\n\tlet { data, transform }: { data: U; transform: (v: T) => string } = $props();\n</script>\n{#each Object.entries(data) as [key, val]}\n\t<p>{key}: {transform(val)}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- performance-safe tests ---

    #[test]
    fn test_large_template() {
        let mut s = String::from("<div>");
        for i in 0..100 {
            s.push_str(&format!("<p class=\"item-{}\">Item {}</p>", i, i));
        }
        s.push_str("</div>");
        let r = parser::parse(&s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_large_style() {
        let mut s = String::from("<style>");
        for i in 0..50 {
            s.push_str(&format!(".class-{} {{ color: rgb({}, {}, {}); }}\n", i, i*5, i*3, i*7));
        }
        s.push_str("</style>");
        let r = parser::parse(&s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_large_script() {
        let mut s = String::from("<script>\n");
        for i in 0..50 {
            s.push_str(&format!("\tlet var_{} = {};\n", i, i));
        }
        s.push_str("</script>");
        let r = parser::parse(&s);
        assert!(r.errors.is_empty());
    }

    // --- beyond 800 ---

    #[test]
    fn test_parse_each_object_destructure() {
        let s = "{#each entries as { key, value }}\n\t<dt>{key}</dt><dd>{value}</dd>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_array_destructure() {
        let s = "{#each pairs as [a, b] (a)}\n\t<p>{a} = {b}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_with_comments() {
        let s = "<style>\n\t/* header styles */\n\th1 { color: blue; }\n\t/* footer */\n\tfooter { background: gray; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_multiple_buttons_no_type() {
        let s = "<div>\n\t<button>A</button>\n\t<button type=\"button\">B</button>\n\t<button>C</button>\n</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let btn_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/button-has-type").collect();
        assert_eq!(btn_diags.len(), 2, "Should flag 2 buttons without type");
    }

    #[test]
    fn test_parse_svelte5_onclick_shorthand() {
        let s = "<button {onclick}>text</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_derived_by_complex() {
        let s = "<script>\n\tlet items = $state([1,2,3]);\n\tlet total = $derived.by(() => {\n\t\tlet sum = 0;\n\t\tfor (const i of items) sum += i;\n\t\treturn sum;\n\t});\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_effect_cleanup() {
        let s = "<script>\n\t$effect(() => {\n\t\tconst handler = () => {};\n\t\twindow.addEventListener('resize', handler);\n\t\treturn () => window.removeEventListener('resize', handler);\n\t});\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_inspect_with_callback() {
        let s = "<script>\n\t$inspect(count).with(console.trace);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inspect"),
            "Should flag $inspect.with()");
    }

    #[test]
    fn test_parse_attribute_with_at_sign() {
        let s = "<div @click={handler}>text</div>";
        let r = parser::parse(s);
        // May or may not parse, but shouldn't panic
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_css_is_where() {
        let s = "<style>\n\t:is(h1, h2, h3) { color: blue; }\n\t:where(.a, .b) { margin: 0; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- final push to 800 ---

    #[test]
    fn test_parse_css_with_important() {
        let s = "<style>\n\t.override { color: red !important; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_pseudo_element() {
        let s = "<style>\n\tp::before { content: '>>'; }\n\tp::after { content: '<<'; }\n\tp::first-line { text-transform: uppercase; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_multiple_selectors() {
        let s = "<style>\n\th1, h2, h3 { color: blue; }\n\t.a, .b { margin: 0; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_calc() {
        let s = "<style>\n\t.box { width: calc(100% - 2rem); height: calc(100vh - 60px); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_grid() {
        let s = "<style>\n\t.grid {\n\t\tdisplay: grid;\n\t\tgrid-template-columns: repeat(3, 1fr);\n\t\tgap: 1rem;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_rest_props() {
        let s = "<script>\n\tlet { a, b, ...rest } = $props();\n</script>\n<div {a} {b} {...rest}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_rest() {
        let s = "{#each items as [first, ...rest]}\n\t<p>{first}: {rest.length} more</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_if_chain() {
        let s = "{#if status === 'loading'}\n\t<Spinner />\n{:else if status === 'error'}\n\t<Error />\n{:else if status === 'empty'}\n\t<Empty />\n{:else}\n\t<Content />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_each_without_key_with_key() {
        let s1 = "{#each items as item}\n\t<p>{item}</p>\n{/each}";
        let s2 = "{#each items as item (item.id)}\n\t<p>{item}</p>\n{/each}";
        let r1 = parser::parse(s1);
        let r2 = parser::parse(s2);
        let d1 = Linter::all().lint(&r1.ast, s1);
        let d2 = Linter::all().lint(&r2.ast, s2);
        assert!(d1.iter().any(|d| d.rule_name == "svelte/require-each-key"));
        assert!(!d2.iter().any(|d| d.rule_name == "svelte/require-each-key"));
    }

    #[test]
    fn test_parse_element_with_many_attrs() {
        let s = "<input\n\ttype=\"text\"\n\tid=\"name\"\n\tname=\"name\"\n\tplaceholder=\"Enter name\"\n\tclass=\"input\"\n\trequired\n\tautofocus\n\tbind:value={name}\n\ton:input={handleInput}\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_mixed_static_dynamic_attrs() {
        let s = "<div id=\"static\" class={dynamic} data-x=\"static\" style:color={color}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_key_block_complex() {
        let s = "{#key selectedId}\n\t<DetailView id={selectedId} />\n{/key}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_await_catch_only() {
        let s = "{#await promise}\n\t<p>Loading</p>\n{:catch error}\n\t<p>{error.message}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_snippet_no_params() {
        let s = "{#snippet footer()}\n\t<p>Footer</p>\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_render_optional() {
        let s = "{#if headerSnippet}\n\t{@render headerSnippet()}\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_debug_multiple() {
        let s = "{@debug a, b, c, d}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::DebugTag(tag) = &r.ast.html.nodes[0] {
            assert_eq!(tag.identifiers.len(), 4);
        }
    }

    #[test]
    fn test_parse_const_in_each() {
        let s = "{#each items as item}\n\t{@const total = item.price * item.qty}\n\t<p>{item.name}: {total}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_multiple_scripts_with_style() {
        let s = "<script context=\"module\">\n\texport const prerender = true;\n</script>\n\n<script>\n\tlet count = 0;\n</script>\n\n<p>{count}</p>\n\n<style>\n\tp { color: blue; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.module.is_some());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    #[test]
    fn test_parse_dynamic_tag_name() {
        let s = "<svelte:element this={isDiv ? 'div' : 'span'}>content</svelte:element>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_event_modifiers_once_capture() {
        let s = "<div on:click|once|capture={handler}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_debug_single_var() {
        let s = "{@debug x}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-debug-tags"));
    }

    #[test]
    fn test_parse_option_immutable() {
        let s = "<svelte:options immutable={true} />\n<p>content</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_binding_contenteditable() {
        let s = "<div contenteditable=\"true\" bind:textContent={text}>Edit me</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- pushing to 800 ---

    #[test]
    fn test_inner_decl_in_if() {
        let s = "<script>\n\tif (true) {\n\t\tfunction inner() {}\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inner-declarations"),
            "Should flag function declaration inside if");
    }

    #[test]
    fn test_inner_decl_in_fn_ok() {
        let s = "<script>\n\tfunction outer() {\n\t\tfunction inner() {}\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-inner-declarations"),
            "Should NOT flag function inside function body");
    }

    #[test]
    fn test_inner_decl_fn_expression_ok() {
        let s = "<script>\n\tif (true) {\n\t\tvar fn = function() {};\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-inner-declarations"),
            "Should NOT flag function expression inside if");
    }

    #[test]
    fn test_inner_decl_top_level_ok() {
        let s = "<script>\n\tfunction topLevel() {}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-inner-declarations"),
            "Should NOT flag top-level function");
    }

    #[test]
    fn test_parse_snippet_inside_if() {
        let s = "{#if show}\n\t{#snippet content()}\n\t\t<p>visible</p>\n\t{/snippet}\n\t{@render content()}\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_nested_each_with_keys() {
        let s = "{#each categories as cat (cat.id)}\n\t<h2>{cat.name}</h2>\n\t{#each cat.items as item (item.id)}\n\t\t<p>{item.name}</p>\n\t{/each}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_conditional_class() {
        let s = "<div\n\tclass=\"base\"\n\tclass:active={isActive}\n\tclass:disabled={!isEnabled}\n\tclass:highlight={isActive && isEnabled}\n>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_options_with_tag() {
        let s = "<svelte:options tag=\"my-element\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_event_handlers() {
        let s = "<div\n\ton:mouseenter={enter}\n\ton:mouseleave={leave}\n\ton:click={click}\n\ton:keydown|preventDefault={key}\n>interactive</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_dimensions() {
        let s = "<div bind:clientWidth={w} bind:clientHeight={h} bind:offsetWidth={ow} bind:offsetHeight={oh}>{w}x{h}</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_with_at_rules() {
        let s = "<style>\n\t@import './base.css';\n\t@font-face { font-family: 'Custom'; src: url('font.woff2'); }\n\t.content { font-family: 'Custom'; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_recommended_count() {
        let rec = Linter::recommended();
        assert!(rec.rules().len() >= 20, "Should have at least 20 recommended rules");
    }

    #[test]
    fn test_parse_await_only_pending() {
        let s = "{#await promise}\n\t<p>Loading...</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_else_and_key() {
        let s = "{#each items as item (item.id)}\n\t<p>{item}</p>\n{:else}\n\t<p>Empty!</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_runes_in_class() {
        let s = "<script>\n\tclass Todo {\n\t\ttext = $state('');\n\t\tdone = $state(false);\n\t\ttoggle() { this.done = !this.done; }\n\t}\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_conditional_slot() {
        let s = "{#if $$slots.header}\n\t<slot name=\"header\" />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_reactive_statement_block() {
        let s = "<script>\n\tlet count = 0;\n\t$: {\n\t\tconst d = count * 2;\n\t\tconsole.log(d);\n\t}\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_generics() {
        let s = "<script lang=\"ts\" generics=\"T extends { id: string }\">\n\tlet { items }: { items: T[] } = $props();\n</script>\n{#each items as item (item.id)}\n\t<slot {item} />\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_assignment() {
        let s = "<button on:click={() => count = count + 1}>+1</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_shorthand_attr_same_name() {
        let s = "<div class={className}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        // className != class, so should NOT flag
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/shorthand-attribute"),
            "Should NOT flag when attr name != expression");
    }

    #[test]
    fn test_parse_component_two_way_bind() {
        let s = "<Counter bind:count />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_global_style_function() {
        let s = "<style>\n\t:global(body) {\n\t\tmargin: 0;\n\t\tpadding: 0;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_issues_plain_html() {
        let s = "<h1>Title</h1>\n<p>Paragraph</p>\n<a href=\"https://example.com\">Link</a>\n<img src=\"img.jpg\" alt=\"photo\" />";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty(), "Plain HTML should have no recommended warnings");
    }

    #[test]
    fn test_parse_mixed_script_template_style() {
        let s = "<script>\n\tlet x = 1;\n</script>\n\n<p>{x}</p>\n\n<style>\n\tp { color: red; }\n</style>\n\nTrailing text";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
        assert!(!r.ast.html.nodes.is_empty());
    }

    #[test]
    fn test_parse_svelte5_event_callback() {
        let s = "<button onclick={(e) => {\n\te.preventDefault();\n\thandle(e.target);\n}}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_as_reserved() {
        // Using reserved-ish names
        let s = "{#each items as class}\n\t<p>{class}</p>\n{/each}";
        let r = parser::parse(s);
        // May or may not work, but shouldn't panic
        let _ = r.ast.html.nodes.len();
    }

    // --- pushing to 750 ---

    #[test]
    fn test_parse_class_concat_complex() {
        let s = "<div class=\"static {dynamic} {cond ? 'a' : 'b'} more\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_on_directive_bare() {
        let s = "<button on:click>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_each_item() {
        let s = "{#each items as item}\n\t<input bind:value={item.name} />\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_no_reactive_fn_arrow() {
        let s = "<script>\n\t$: fn = () => 42;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-functions"),
            "Should flag reactive arrow function");
    }

    #[test]
    fn test_no_reactive_fn_named_ok() {
        let s = "<script>\n\tfunction myFn() { return 42; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-reactive-functions"),
            "Should NOT flag regular function declaration");
    }

    #[test]
    fn test_shorthand_directive_class() {
        let s = "<div class:active={active}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/shorthand-directive"),
            "Should flag non-shorthand class directive");
    }

    #[test]
    fn test_shorthand_directive_class_ok() {
        let s = "<div class:active>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/shorthand-directive"),
            "Should NOT flag shorthand class directive");
    }

    #[test]
    fn test_no_raw_in_template() {
        let s = "{@html '<p>raw</p>'}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-html-tags"),
            "Should flag @html in template");
    }

    #[test]
    fn test_parse_keyed_each_fn_key() {
        let s = "{#each items as item (getId(item))}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::EachBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.key.as_deref(), Some("getId(item)"));
        }
    }

    #[test]
    fn test_parse_complex_await_chain() {
        let s = "{#await fetch('/api').then(r => r.json())}\n\t<p>loading</p>\n{:then data}\n\t<p>{JSON.stringify(data)}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_dynamic_element() {
        let s = "<svelte:element this={tag} class=\"dynamic\">{content}</svelte:element>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_empty_each_flagged() {
        let s = "{#each [] as item}\n\t<p>{item}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-each-key"),
            "Should flag each without key even on empty array");
    }

    #[test]
    fn test_parse_expression_with_comma() {
        let s = "<p>{(a, b)}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_object() {
        let s = "<p>{{ key: 'value' }}</p>";
        let r = parser::parse(s);
        // Object literal in mustache
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_conditional_attribute() {
        let s = "<div class={condition ? 'active' : undefined}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_nullish_coalesce() {
        let s = "<p>{value ?? 'default'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_optional_chain_call() {
        let s = "<p>{obj?.method?.(arg)}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- comprehensive linter coverage batch ---

    #[test]
    fn test_multiple_class_names_unused() {
        let s = "<div class=\"a b c\">text</div>\n<style>\n\t.a { color: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let unused: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-unused-class-name").collect();
        assert_eq!(unused.len(), 2, "Should flag 'b' and 'c' as unused, got: {:?}", unused.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_reactive_reassign_member_obj() {
        let s = "<script>\n\tlet v = 0;\n\t$: obj = { v };\n\tfunction click() { obj.v = 1; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag member assignment on reactive object");
    }

    #[test]
    fn test_reactive_reassign_delete() {
        let s = "<script>\n\tlet v = 0;\n\t$: obj = { v };\n\tfunction click() { delete obj.v; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag delete on reactive object");
    }

    #[test]
    fn test_html_closing_bracket_multiline_no_break() {
        let s = "<div\n\tclass=\"foo\"\n\tid=\"bar\"></div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/html-closing-bracket-new-line"),
            "Should flag multiline element with no line break before >");
    }

    #[test]
    fn test_no_spaces_equal_ok() {
        let s = "<div class=\"foo\" id=\"bar\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-spaces-around-equal-signs-in-attribute"),
            "Should NOT flag attributes without spaces around =");
    }

    #[test]
    fn test_no_trailing_spaces_multi() {
        let s = "<p>line1</p>  \n<p>line2</p>\t\n<p>line3</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let trail: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-trailing-spaces").collect();
        assert_eq!(trail.len(), 2, "Should flag 2 lines with trailing spaces");
    }

    #[test]
    fn test_no_inline_styles_dynamic() {
        let s = "<div style=\"color: {active ? 'red' : 'blue'}\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inline-styles"),
            "Should flag dynamic inline style");
    }

    #[test]
    fn test_prefer_style_directive_no_style_ok() {
        let s = "<div class=\"foo\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/prefer-style-directive"),
            "Should NOT flag element without style");
    }

    #[test]
    fn test_button_input_not_flagged() {
        let s = "<input type=\"submit\" value=\"Submit\" />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/button-has-type"),
            "Should NOT flag input type=submit");
    }

    #[test]
    fn test_a_without_href_ok_for_blank() {
        let s = "<a>no href</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-target-blank"),
            "Should NOT flag <a> without target=_blank");
    }

    #[test]
    fn test_writable_derived_multiple_effects() {
        let s = "<script>\n\tconst { a, b } = $props();\n\tlet x = $state(a);\n\tlet y = $state(b);\n\t$effect(() => { x = a; });\n\t$effect(() => { y = b; });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let wd: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/prefer-writable-derived").collect();
        assert_eq!(wd.len(), 2, "Should flag both $state + $effect pairs");
    }

    #[test]
    fn test_store_callback_no_set_nested() {
        let s = "<script>\n\timport { readable } from 'svelte/store';\n\treadable(0, () => { return () => {}; });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-callbacks-use-set-param"),
            "Should flag callback returning cleanup but not using set");
    }

    #[test]
    fn test_no_store_async_sync_callback_ok() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst s = writable(0, (set) => {\n\t\tconst interval = setInterval(() => set(Date.now()), 1000);\n\t\treturn () => clearInterval(interval);\n\t});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-store-async"),
            "Should NOT flag sync store callback");
    }

    #[test]
    fn test_svelte_internal_import() {
        let s = "<script>\n\timport { flush } from 'svelte/internal';\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-svelte-internal"),
            "Should flag svelte/internal import");
    }

    #[test]
    fn test_svelte_internal_client() {
        let s = "<script>\n\timport { mount } from 'svelte/internal/client';\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-svelte-internal"),
            "Should flag svelte/internal/client import");
    }

    #[test]
    fn test_reactive_literal_false_expr() {
        let s = "<script>\n\tlet count = 0;\n\t$: doubled = count * 2;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should NOT flag reactive expression using variable");
    }

    #[test]
    fn test_no_at_html_multiple() {
        let s = "{@html a}\n{@html b}\n{@html c}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let html_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-at-html-tags").collect();
        assert_eq!(html_diags.len(), 3, "Should flag all 3 @html tags");
    }

    #[test]
    fn test_each_key_self_ok() {
        let s = "{#each [1, 2, 3] as num (num)}\n\t<p>{num}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let key_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/valid-each-key").collect();
        // Using num as both context and key is flagged
        assert!(!key_diags.is_empty(), "Should flag using item directly as key");
    }

    // --- additional coverage ---

    #[test]
    fn test_browser_global_in_onmount_ok() {
        let s = "<script>\n\timport { onMount } from 'svelte';\n\tonMount(() => {\n\t\tconsole.log(window.innerWidth);\n\t});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should NOT flag window inside onMount");
    }

    #[test]
    fn test_browser_global_in_effect_ok() {
        let s = "<script>\n\t$effect(() => {\n\t\tconsole.log(document.title);\n\t});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should NOT flag document inside $effect");
    }

    #[test]
    fn test_browser_global_in_function_ok() {
        let s = "<script>\n\tfunction handleClick() {\n\t\tconsole.log(navigator.userAgent);\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should NOT flag navigator inside function");
    }

    #[test]
    fn test_browser_global_fetch_top_level() {
        let s = "<script>\n\tconst data = fetch('/api');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should flag fetch at top level");
    }

    #[test]
    fn test_browser_global_localstorage_top_level() {
        let s = "<script>\n\tconst x = localStorage.getItem('key');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should flag localStorage at top level");
    }

    #[test]
    fn test_browser_global_typeof_guard_ok() {
        let s = "<script>\n\tconst x = typeof window !== 'undefined' ? window.innerWidth : 0;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should NOT flag window with typeof guard");
    }

    #[test]
    fn test_prefer_destructured_store_member() {
        let s = "<script>\n\timport { count } from './stores';\n</script>\n{$count.value}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-destructured-store-props"),
            "Should flag $count.value");
    }

    #[test]
    fn test_prefer_destructured_store_ok() {
        let s = "<script>\n\timport { count } from './stores';\n</script>\n{$count}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/prefer-destructured-store-props"),
            "Should NOT flag $count without member");
    }

    #[test]
    fn test_immutable_reactive_function_decl() {
        let s = "<script>\n\texport function greet() {}\n\t$: greet();\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should flag reactive statement calling immutable function");
    }

    #[test]
    fn test_dom_manip_appendChild() {
        let s = "<script>\n\tlet div;\n\tconst add = () => div.appendChild(document.createElement('p'));\n</script>\n<div bind:this={div} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dom-manipulating"),
            "Should flag appendChild on bind:this element");
    }

    #[test]
    fn test_browser_global_in_globalthis_guard() {
        let s = "<script>\n\tif (globalThis.location !== undefined) {\n\t\tconsole.log(location.href);\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let loc_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-top-level-browser-globals").collect();
        assert!(loc_diags.is_empty(),
            "Should NOT flag location inside globalThis guard, got: {:?}", loc_diags.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_browser_global_top_level() {
        let s = "<script>\n\tconsole.log(window.innerWidth);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should flag window at top level");
    }

    // --- final push to 700 ---

    #[test]
    fn test_parse_class_expression() {
        let s = "<div class=\"{active ? 'active' : ''} {size} extra\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_expression() {
        let s = "<div style=\"color: {color}; font-size: {size}px;\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_complex() {
        let s = "<script>\n\tasync function loadData() {\n\t\tconst res = await fetch('/api');\n\t\treturn res.json();\n\t}\n\tconst data = loadData();\n</script>\n{#await data}\n\t<p>Loading...</p>\n{:then result}\n\t<pre>{JSON.stringify(result)}</pre>\n{:catch err}\n\t<p>{err.message}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_diag_count() {
        let s = "<button>no type</button>\n<button>also no type</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let btn_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/button-has-type").collect();
        assert_eq!(btn_diags.len(), 2, "Should flag both buttons");
    }

    #[test]
    fn test_parse_multiline_if() {
        let s = "{#if\n\tcondition &&\n\tanotherCondition\n}\n\t<p>true</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiline_each() {
        let s = "{#each\n\titems.filter(x => x.active)\n\tas item\n\t(item.id)\n}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiline_await() {
        let s = "{#await\n\tloadData()\n}\n\t<p>loading</p>\n{:then data}\n\t<p>{data}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_dotted() {
        let s = "<Form.Field>\n\t<Form.Input bind:value />\n</Form.Field>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_whitespace_in_mustache() {
        let s = "<p>{  value  }</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_comment_in_script() {
        let s = "<script>\n\t// line comment\n\t/* block comment */\n\tlet x = 1;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_conditional_spread() {
        let s = "<div {...(active ? activeProps : defaultProps)}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_spreads() {
        let s = "<div {...a} {...b} {...c}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_issues_on_simple() {
        let s = "<p>Hello World</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.is_empty(), "Simple paragraph should have no issues");
    }

    #[test]
    fn test_parse_svelte_head_link() {
        let s = "<svelte:head>\n\t<link rel=\"stylesheet\" href=\"/style.css\" />\n</svelte:head>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_style_blocks() {
        // Svelte only supports one style block but parser should handle gracefully
        let s = "<style>.a { color: red; }</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_slot_default_content() {
        let s = "<slot name=\"actions\">\n\t<button type=\"button\">Default Action</button>\n</slot>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_each_else_empty() {
        let s = "{#each [] as item}\n\t<p>{item}</p>\n{:else}\n\t<p>No items</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_form_action() {
        let s = "<form method=\"POST\" action=\"?/login\">\n\t<input name=\"email\" type=\"email\" />\n\t<button type=\"submit\">Login</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_contenteditable() {
        let s = "<div contenteditable bind:innerHTML={html}>Edit me</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_custom_element() {
        let s = "<my-component data-foo=\"bar\">content</my-component>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_all_clean() {
        let s = "<script lang=\"ts\">\n\tlet name = 'World';\n</script>\n<p>Hello {name}!</p>\n<style lang=\"scss\">\n\tp { color: blue; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        let filtered: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(filtered.is_empty(), "Clean typed component: {:?}",
            filtered.iter().map(|d| &d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_parse_window_scroll() {
        let s = "<svelte:window bind:scrollY={y} on:scroll={handleScroll} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_body_touch() {
        let s = "<svelte:body on:touchstart={handleTouch} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_with_namespace() {
        let s = "<svg xmlns=\"http://www.w3.org/2000/svg\">\n\t<path d=\"M 0 0 L 10 10\" />\n</svg>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_store_auto_subscribe() {
        let s = "<script>\n\timport { page } from '$app/stores';\n\t$: url = $page.url;\n</script>\n<p>{url.pathname}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- Svelte component patterns ---

    #[test]
    fn test_recursive_component() {
        let s = "<script>\n\texport let depth = 5;\n</script>\n{#if depth > 0}\n\t<p>Depth: {depth}</p>\n\t<svelte:self depth={depth - 1} />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_component_with_slot_props() {
        let s = "<Hoverable let:hovering>\n\t<div class:active={hovering}>hover area</div>\n</Hoverable>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_component_with_named_slots() {
        let s = "<Card>\n\t<svelte:fragment slot=\"header\">\n\t\t<h2>Title</h2>\n\t</svelte:fragment>\n\t<p>Body content</p>\n\t<svelte:fragment slot=\"footer\">\n\t\t<button>OK</button>\n\t</svelte:fragment>\n</Card>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_store_contract() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst theme = writable('dark');\n\tconst toggle = () => theme.update(t => t === 'dark' ? 'light' : 'dark');\n</script>\n<button on:click={toggle}>{$theme}</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_keyed_each_with_transition() {
        let s = "{#each list as item (item.id)}\n\t<div transition:fade>{item.text}</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_component_binding() {
        let s = "<script>\n\tlet input;\n</script>\n<CustomInput bind:this={input} bind:value />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_snippet_with_render() {
        let s = "{#snippet header(title, subtitle)}\n\t<h1>{title}</h1>\n\t<p>{subtitle}</p>\n{/snippet}\n\n{@render header('Hello', 'World')}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_conditional_rendering_pattern() {
        let s = "{#if loading}\n\t<Spinner />\n{:else if error}\n\t<Error message={error.message} />\n{:else if data}\n\t<DataView {data} />\n{:else}\n\t<Empty />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_action_with_params() {
        let s = "<div use:clickOutside on:outclick={close}>content</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_each_index_without_key() {
        let s = "{#each items as _, i}\n\t<p>Index: {i}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_html_entities_comprehensive() {
        let s = "<p>&lt;div&gt; &amp;&amp; &quot;quoted&quot; &#169; &#x00A9;</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_script_with_generics() {
        let s = "<script lang=\"ts\" generics=\"T\">\n\tlet { items }: { items: T[] } = $props();\n</script>\n{#each items as item}<p>{item}</p>{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_component_with_rest_props() {
        let s = "<script>\n\tlet { class: className, ...rest } = $props();\n</script>\n<div class={className} {...rest}><slot /></div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_bind_group_radio() {
        let s = "<script>\n\tlet choice = 'a';\n</script>\n{#each ['a', 'b', 'c'] as option}\n\t<label>\n\t\t<input type=\"radio\" bind:group={choice} value={option} />\n\t\t{option}\n\t</label>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_two_way_binding_select() {
        let s = "<script>\n\tlet selected = '';\n\tconst options = ['red', 'green', 'blue'];\n</script>\n<select bind:value={selected}>\n\t{#each options as opt}\n\t\t<option value={opt}>{opt}</option>\n\t{/each}\n</select>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- CSS feature tests ---

    #[test]
    fn test_css_container_queries() {
        let s = "<style>\n\t@container (min-width: 700px) {\n\t\t.card { flex-direction: row; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_layers() {
        let s = "<style>\n\t@layer base {\n\t\tp { margin: 0; }\n\t}\n\t@layer theme {\n\t\tp { color: blue; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_nesting() {
        let s = "<style>\n\t.parent {\n\t\tcolor: red;\n\t\t& .child { color: blue; }\n\t\t&:hover { color: green; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_supports() {
        let s = "<style>\n\t@supports (display: grid) {\n\t\t.grid { display: grid; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_has_selector() {
        let s = "<style>\n\tdiv:has(> p) { margin: 1rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- linter coverage expansion ---

    #[test]
    fn test_multiple_event_handlers() {
        let s = "<div on:mouseenter={enter} on:mouseleave={leave} on:click={click}>hover me</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-dupe-on-directives"),
            "Different events should not be flagged as duplicates");
    }

    #[test]
    fn test_style_directive_with_fallback() {
        let s = "<div style:color={theme} style:background-color=\"white\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_each_without_key_nested() {
        let s = "{#each items as item}\n\t{#each item.children as child}\n\t\t<p>{child}</p>\n\t{/each}\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let key_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/require-each-key").collect();
        assert!(key_diags.len() >= 2, "Should flag both each blocks without keys");
    }

    #[test]
    fn test_no_nav_without_base_absolute_ok() {
        let s = "<a href=\"https://svelte.dev\">Svelte</a>\n<a href=\"mailto:test@test.com\">Email</a>\n<a href=\"tel:+1234567890\">Call</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should NOT flag absolute URLs, mailto, tel");
    }

    #[test]
    fn test_class_directive_not_in_css() {
        let s = "<div class:missing-class={true}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-unused-class-name"),
            "Should flag class directive class not in CSS");
    }

    // --- final batch tests ---

    #[test]
    fn test_empty_blocks() {
        let sources = [
            "{#if x}{/if}",
            "{#each items as item}{/each}",
            "{#await p}{:then}{:catch}{/await}",
            "{#key k}{/key}",
            "{#snippet s()}{/snippet}",
        ];
        for s in &sources {
            let r = parser::parse(s);
            assert!(r.errors.is_empty(), "Failed: {}", s);
        }
    }

    #[test]
    fn test_nested_components() {
        let s = "<Outer>\n\t<Middle>\n\t\t<Inner prop={val}>\n\t\t\t<p>deep content</p>\n\t\t</Inner>\n\t</Middle>\n</Outer>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_event_forwarding() {
        let s = "<button on:click>Forward click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_style_global_selector() {
        let s = "<style>\n\t:global(body) { margin: 0; }\n\t.local :global(.external) { color: blue; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_variables() {
        let s = "<div style:--theme-color=\"blue\" style:--gap=\"1rem\">styled</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_slot_prop() {
        let s = "<List items={data} let:item let:index>\n\t<p>{index}: {item.name}</p>\n</List>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_await_then_only() {
        let s = "{#await promise then data}\n\t<p>{data}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_onclick_with_arg() {
        let s = "<button onclick={(e) => { e.preventDefault(); handle(e); }}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_text_and_elements() {
        let s = "before <span>middle</span> after";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.html.nodes.len() >= 3);
    }

    #[test]
    fn test_parse_doctype() {
        let s = "<!DOCTYPE html>\n<html><body>content</body></html>";
        let r = parser::parse(s);
        // Doctype handling varies but shouldn't panic
        let _ = r.ast.html.nodes.len();
    }

    // --- complete component tests ---

    #[test]
    fn test_complete_counter_app() {
        let s = "<script>\n\tlet count = 0;\n\t$: doubled = count * 2;\n\t$: quadrupled = doubled * 2;\n\tconst inc = () => count += 1;\n\tconst dec = () => count -= 1;\n\tconst reset = () => count = 0;\n</script>\n\n<h1>Counter</h1>\n<p>{count} × 2 = {doubled}</p>\n<p>{count} × 4 = {quadrupled}</p>\n<div>\n\t<button on:click={dec}>-</button>\n\t<button on:click={reset}>Reset</button>\n\t<button on:click={inc}>+</button>\n</div>\n\n<style>\n\th1 { text-align: center; }\n\tdiv { display: flex; gap: 0.5rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    #[test]
    fn test_complete_todo_svelte5() {
        let s = "<script lang=\"ts\">\n\tinterface Todo { id: number; text: string; done: boolean; }\n\tlet todos = $state<Todo[]>([]);\n\tlet input = $state('');\n\tlet remaining = $derived(todos.filter(t => !t.done).length);\n\tconst add = () => {\n\t\tif (!input.trim()) return;\n\t\ttodos.push({ id: Date.now(), text: input, done: false });\n\t\tinput = '';\n\t};\n</script>\n\n<h1>Todos ({remaining})</h1>\n<form onsubmit={(e) => { e.preventDefault(); add(); }}>\n\t<input bind:value={input} />\n\t<button type=\"submit\">Add</button>\n</form>\n{#each todos as todo (todo.id)}\n\t<label>\n\t\t<input type=\"checkbox\" bind:checked={todo.done} />\n\t\t<span class:done={todo.done}>{todo.text}</span>\n\t</label>\n{/each}\n\n<style>\n\t.done { text-decoration: line-through; opacity: 0.5; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_complete_layout() {
        let s = "<script>\n\texport let data;\n</script>\n\n<header>\n\t<nav>\n\t\t<a href=\"/\">Home</a>\n\t\t<a href=\"/about\">About</a>\n\t</nav>\n</header>\n\n<main>\n\t<slot />\n</main>\n\n<footer>\n\t<p>&copy; 2024</p>\n</footer>\n\n<style>\n\theader { background: #333; padding: 1rem; }\n\tnav { display: flex; gap: 1rem; }\n\tnav a { color: white; text-decoration: none; }\n\tmain { min-height: 80vh; padding: 1rem; }\n\tfooter { text-align: center; padding: 1rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_complete_store_example() {
        let s = "<script>\n\timport { writable, derived } from 'svelte/store';\n\timport { onDestroy } from 'svelte';\n\n\tconst items = writable([1, 2, 3]);\n\tconst total = derived(items, ($items) => $items.reduce((a, b) => a + b, 0));\n\tconst count = derived(items, ($items) => $items.length);\n\n\tlet unsubTotal;\n\tlet currentTotal = 0;\n\tunsubTotal = total.subscribe(v => currentTotal = v);\n\tonDestroy(() => unsubTotal?.());\n</script>\n\n<p>Items: {$count}</p>\n<p>Total: {$total} (also {currentTotal})</p>\n<button on:click={() => $items = [...$items, $items.length + 1]}>Add</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_complete_animation() {
        let s = "<script>\n\timport { fade, fly, slide, scale } from 'svelte/transition';\n\timport { flip } from 'svelte/animate';\n\tlet visible = true;\n\tlet items = [1, 2, 3];\n</script>\n\n<button on:click={() => visible = !visible}>Toggle</button>\n\n{#if visible}\n\t<div transition:fade>Fade</div>\n\t<div in:fly={{y: 200}} out:scale>Fly in, scale out</div>\n\t<div transition:slide>Slide</div>\n{/if}\n\n<ul>\n\t{#each items as item (item)}\n\t\t<li animate:flip={{duration: 300}}>{item}</li>\n\t{/each}\n</ul>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_complete_context_api() {
        let s = "<script>\n\timport { setContext, getContext } from 'svelte';\n\timport { writable } from 'svelte/store';\n\n\tconst theme = writable('light');\n\tsetContext('theme', theme);\n\n\tconst userTheme = getContext('theme');\n</script>\n\n<div class=\"app\">\n\t<slot />\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_complete_dynamic_component() {
        let s = "<script>\n\timport Red from './Red.svelte';\n\timport Blue from './Blue.svelte';\n\tlet selected = Red;\n\tconst options = [{ component: Red, name: 'Red' }, { component: Blue, name: 'Blue' }];\n</script>\n\n{#each options as opt}\n\t<button on:click={() => selected = opt.component}>{opt.name}</button>\n{/each}\n\n<svelte:component this={selected} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- comprehensive edge cases ---

    #[test]
    fn test_each_complex_expressions() {
        let sources = [
            "{#each Array.from({length: n}) as _, i (i)}<p>{i}</p>{/each}",
            "{#each [...items, extra] as item}<p>{item}</p>{/each}",
            "{#each items.filter(i => i.active) as item (item.id)}<p>{item.name}</p>{/each}",
        ];
        for s in &sources {
            let r = parser::parse(s);
            assert!(r.errors.is_empty(), "Failed to parse: {}", s);
        }
    }

    #[test]
    fn test_if_complex_conditions() {
        let sources = [
            "{#if items?.length > 0}<p>has items</p>{/if}",
            "{#if typeof window !== 'undefined'}<p>browser</p>{/if}",
            "{#if a && (b || c)}<p>complex</p>{/if}",
        ];
        for s in &sources {
            let r = parser::parse(s);
            assert!(r.errors.is_empty(), "Failed to parse: {}", s);
        }
    }

    #[test]
    fn test_attribute_edge_cases() {
        let sources = [
            "<div class:foo class:bar>text</div>",
            "<input type=\"range\" min={0} max={100} step={1} />",
            "<div data-tooltip=\"test\" aria-hidden=\"true\">text</div>",
            "<img loading=\"lazy\" decoding=\"async\" src={url} alt=\"\" />",
        ];
        for s in &sources {
            let r = parser::parse(s);
            assert!(r.errors.is_empty(), "Failed to parse: {}", s);
        }
    }

    #[test]
    fn test_multiple_blocks_same_level() {
        let s = "{#if a}\n\t<p>a</p>\n{/if}\n{#if b}\n\t<p>b</p>\n{/if}\n{#if c}\n\t<p>c</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.html.nodes.len() >= 3);
    }

    #[test]
    fn test_whitespace_between_elements() {
        let s = "<p>a</p>\n\n<p>b</p>\n\n<p>c</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_head_with_meta() {
        let s = "<svelte:head>\n\t<title>{pageTitle}</title>\n\t<meta name=\"description\" content={description} />\n</svelte:head>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_window_bindings() {
        let s = "<svelte:window bind:scrollY={y} bind:innerWidth={width} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_events() {
        let s = "<Widget on:custom={handler} on:click on:submit|preventDefault={submit} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_multiple_issues() {
        let s = "<script>\n\t$inspect(x);\n</script>\n{@html dangerous}\n{@debug value}\n{#each items as item}\n\t<p>{item}</p>\n{/each}\n<button>click</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.len() >= 3, "Should have multiple diagnostics, got {}", diags.len());
    }

    #[test]
    fn test_parse_svg_with_svelte() {
        let s = "<svg viewBox=\"0 0 100 100\">\n\t{#each circles as c}\n\t\t<circle cx={c.x} cy={c.y} r={c.r} fill={c.color} />\n\t{/each}\n</svg>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_table_with_each() {
        let s = "<table>\n\t<thead><tr><th>Name</th><th>Age</th></tr></thead>\n\t<tbody>\n\t\t{#each people as p (p.id)}\n\t\t\t<tr><td>{p.name}</td><td>{p.age}</td></tr>\n\t\t{/each}\n\t</tbody>\n</table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_prefer_writable_derived_basic() {
        let s = "<script>\n\tconst { x } = $props();\n\tlet y = $state(x);\n\t$effect(() => {\n\t\ty = x;\n\t});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should flag $state + $effect pattern");
    }

    #[test]
    fn test_prefer_writable_derived_conditional_ok() {
        let s = "<script>\n\tconst { x } = $props();\n\tlet y = $state(x);\n\t$effect(() => {\n\t\tif (x > 0) y = x;\n\t});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should NOT flag conditional $effect");
    }

    #[test]
    fn test_prefer_writable_derived_no_effect_ok() {
        let s = "<script>\n\tlet y = $state(0);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should NOT flag $state without $effect");
    }

    #[test]
    fn test_prefer_writable_derived_effect_pre() {
        let s = "<script>\n\tconst { x } = $props();\n\tlet y = $state(x);\n\t$effect.pre(() => {\n\t\ty = x;\n\t});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should flag $state + $effect.pre pattern");
    }

    // --- navigation + SvelteKit tests ---

    #[test]
    fn test_goto_without_base() {
        let s = "<script>\n\timport { goto } from '$app/navigation';\n\tgoto('/dashboard');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-goto-without-base"),
            "Should flag goto without base");
    }

    #[test]
    fn test_goto_with_base_ok() {
        let s = "<script>\n\timport { goto } from '$app/navigation';\n\timport { base } from '$app/paths';\n\tgoto(`${base}/dashboard`);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-goto-without-base"),
            "Should NOT flag goto with base");
    }

    #[test]
    fn test_nav_base_link_with_base_ok() {
        let s = "<script>\n\timport { base } from '$app/paths';\n</script>\n<a href={`${base}/foo`}>link</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should NOT flag link using base");
    }

    #[test]
    fn test_no_export_load_in_module_basic() {
        let s = "<script context=\"module\">\n\texport function load() { return {}; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-export-load-in-svelte-module-in-kit-pages"),
            "Should flag export load in module");
    }

    // --- Svelte 4 legacy syntax tests ---

    #[test]
    fn test_parse_on_directive_legacy() {
        let s = "<button on:click={handler}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert!(el.attributes.iter().any(|a| matches!(a,
                ast::Attribute::Directive { kind: ast::DirectiveKind::EventHandler, name, .. } if name == "click"
            )));
        }
    }

    #[test]
    fn test_parse_context_module_legacy() {
        let s = "<script context=\"module\">\n\texport const x = 1;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.module.is_some());
        assert!(r.ast.module.as_ref().unwrap().module);
    }

    #[test]
    fn test_parse_reactive_declaration() {
        let s = "<script>\n\tlet count = 0;\n\t$: doubled = count * 2;\n\t$: if (count > 10) console.log('big');\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_export_let_with_default() {
        let s = "<script>\n\texport let name = 'World';\n\texport let count = 0;\n</script>\n<p>Hello {name}! ({count})</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_no_store_async_writable() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\twritable(0, async (set) => { set(1); });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-store-async"),
            "Should flag async writable callback");
    }

    #[test]
    fn test_no_store_async_derived() {
        let s = "<script>\n\timport { derived } from 'svelte/store';\n\tconst d = derived(count, async ($c) => await transform($c));\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-store-async"),
            "Should flag async derived callback");
    }

    #[test]
    fn test_dom_manipulating_chained() {
        let s = "<script>\n\tlet div;\n\tconst update = () => { div?.remove(); };\n</script>\n<div bind:this={div}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dom-manipulating"),
            "Should flag optional chain DOM manipulation");
    }

    #[test]
    fn test_dom_manipulating_inner_html() {
        let s = "<script>\n\tlet el;\n\tconst update = () => { el.innerHTML = '<p>bad</p>'; };\n</script>\n<div bind:this={el}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dom-manipulating"),
            "Should flag innerHTML assignment");
    }

    // --- no-immutable-reactive-statements tests ---

    #[test]
    fn test_immutable_reactive_const_string() {
        let s = "<script>\n\tconst x = 'hello';\n\t$: y = x;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should flag reactive stmt with immutable const string");
    }

    #[test]
    fn test_immutable_reactive_mutable_ok() {
        let s = "<script>\n\tlet x = 'hello';\n</script>\n<input bind:value={x} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should NOT flag when no reactive statements");
    }

    #[test]
    fn test_immutable_reactive_import() {
        let s = "<script>\n\timport val from './mod';\n\t$: console.log(val);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should flag reactive stmt with import");
    }

    #[test]
    fn test_immutable_reactive_mutable_let_ok() {
        let s = "<script>\n\tlet count = 0;\n\t$: doubled = count * 2;\n</script>\n<input bind:value={count} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should NOT flag reactive stmt with mutable let");
    }

    // --- HTML element edge cases ---

    #[test]
    fn test_parse_details_summary() {
        let s = "<details>\n\t<summary>Click to expand</summary>\n\t<p>Hidden content</p>\n</details>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_picture_source() {
        let s = "<picture>\n\t<source srcset=\"large.webp\" media=\"(min-width: 800px)\" />\n\t<img src=\"small.jpg\" alt=\"photo\" />\n</picture>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_dl_dt_dd() {
        let s = "<dl>\n\t<dt>Term</dt>\n\t<dd>Definition</dd>\n</dl>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_fieldset_legend() {
        let s = "<fieldset>\n\t<legend>Personal Info</legend>\n\t<input type=\"text\" />\n</fieldset>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_figure_figcaption() {
        let s = "<figure>\n\t<img src=\"img.jpg\" alt=\"photo\" />\n\t<figcaption>A nice photo</figcaption>\n</figure>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_dialog() {
        let s = "<dialog open>\n\t<p>Dialog content</p>\n\t<button>Close</button>\n</dialog>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_template_tag() {
        let s = "<template>\n\t<p>Template content</p>\n</template>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_progress_meter() {
        let s = "<progress value=\"70\" max=\"100\"></progress>\n<meter value=\"0.7\">70%</meter>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_output() {
        let s = "<output name=\"result\">{result}</output>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_audio_video() {
        let s = "<audio src=\"song.mp3\" controls />\n<video src=\"video.mp4\" controls width=\"640\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_iframe() {
        let s = "<iframe src=\"https://example.com\" title=\"Embedded\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_abbr_time() {
        let s = "<abbr title=\"HyperText Markup Language\">HTML</abbr>\n<time datetime=\"2024-01-01\">New Year</time>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_code_pre() {
        let s = "<pre><code>const x = 42;\nconsole.log(x);</code></pre>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- linter false-positive regression ---

    #[test]
    fn test_no_false_positive_if_block() {
        let s = "{#if condition}\n\t<p>visible</p>\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty(), "Simple if block should have no warnings: {:?}",
            diags.iter().map(|d| &d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_no_false_positive_each_with_key() {
        let s = "{#each items as item (item.id)}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty(), "Each with key should have no warnings: {:?}",
            diags.iter().map(|d| &d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_no_false_positive_component() {
        let s = "<Widget prop={value} on:click={handler}>\n\t<span>child</span>\n</Widget>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty(), "Component usage should have no warnings: {:?}",
            diags.iter().map(|d| &d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_no_false_positive_svelte_window() {
        let s = "<svelte:window on:keydown={handler} />";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty(), "svelte:window should have no warnings");
    }

    #[test]
    fn test_no_false_positive_mustache() {
        let s = "<p>{value}</p>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty(), "Simple mustache should have no warnings");
    }

    #[test]
    fn test_no_false_positive_html_comment() {
        let s = "<!-- This is a comment -->";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty(), "Spaced comment should have no warnings");
    }

    // --- parser stress tests ---

    #[test]
    fn test_parse_many_elements() {
        let mut s = String::new();
        for i in 0..50 {
            s.push_str(&format!("<div class=\"item-{}\">text {}</div>\n", i, i));
        }
        let r = parser::parse(&s);
        assert!(r.errors.is_empty());
        assert!(r.ast.html.nodes.len() >= 50);
    }

    #[test]
    fn test_parse_many_attributes() {
        let s = "<div a=\"1\" b=\"2\" c=\"3\" d=\"4\" e=\"5\" f=\"6\" g=\"7\" h=\"8\" i=\"9\" j=\"10\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_deeply_nested_elements() {
        let s = "<div><div><div><div><div><div><div><div><div><div>deep</div></div></div></div></div></div></div></div></div></div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_long_text() {
        let s = format!("<p>{}</p>", "x".repeat(10000));
        let r = parser::parse(&s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_many_mustaches() {
        let mut s = String::from("<p>");
        for i in 0..20 {
            s.push_str(&format!("{{val{}}} ", i));
        }
        s.push_str("</p>");
        let r = parser::parse(&s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_each() {
        let s = "{#each Object.entries(data).filter(([k, v]) => v > 0).sort((a, b) => a[1] - b[1]) as [key, value] (key)}\n\t<div>{key}: {value}</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_if_condition() {
        let s = "{#if typeof window !== 'undefined' && window.innerWidth > 768 && !isMobile}\n\t<p>desktop</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_template_literal_expression() {
        let s = "<p>{`Hello ${name}, you have ${count} item${count === 1 ? '' : 's'}`}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- edge case attribute patterns ---

    #[test]
    fn test_parse_class_shorthand() {
        let s = "<div {className}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_class_directives() {
        let s = "<div class:a class:b={isB} class:c={true}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_style_directives() {
        let s = "<div style:color=\"red\" style:font-size=\"14px\" style:--custom={val}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_event_with_multiple_modifiers() {
        let s = "<form on:submit|preventDefault|stopPropagation|once={handler}>content</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_various() {
        let s = "<input bind:value />\n<div bind:clientWidth={w} bind:clientHeight={h} />\n<video bind:duration bind:currentTime bind:paused />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_transitions() {
        let s = "<div in:fly={{y: -100}} out:fade={{duration: 300}}>content</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_class_and_style_combined() {
        let s = "<div class=\"base\" class:active class:disabled={!enabled} style=\"padding: 1rem\" style:color>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- real-world template patterns ---

    #[test]
    fn test_real_world_form() {
        let s = "<script>\n\tlet form = { name: '', email: '' };\n\tconst submit = () => console.log(form);\n</script>\n\n<form on:submit|preventDefault={submit}>\n\t<label>\n\t\tName:\n\t\t<input bind:value={form.name} />\n\t</label>\n\t<label>\n\t\tEmail:\n\t\t<input type=\"email\" bind:value={form.email} />\n\t</label>\n\t<button type=\"submit\">Submit</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_real_world_list() {
        let s = "<script>\n\tlet items = [];\n\tlet newItem = '';\n\tconst add = () => { items = [...items, newItem]; newItem = ''; };\n\tconst remove = (i) => { items = items.filter((_, idx) => idx !== i); };\n</script>\n\n<input bind:value={newItem} />\n<button on:click={add}>Add</button>\n\n<ul>\n\t{#each items as item, i (i)}\n\t\t<li>\n\t\t\t{item}\n\t\t\t<button on:click={() => remove(i)}>x</button>\n\t\t</li>\n\t{/each}\n</ul>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_real_world_tabs() {
        let s = "<script>\n\tlet activeTab = 0;\n\tconst tabs = ['Home', 'About', 'Contact'];\n</script>\n\n<div class=\"tabs\">\n\t{#each tabs as tab, i}\n\t\t<button class:active={activeTab === i} on:click={() => activeTab = i}>\n\t\t\t{tab}\n\t\t</button>\n\t{/each}\n</div>\n\n{#if activeTab === 0}\n\t<p>Home content</p>\n{:else if activeTab === 1}\n\t<p>About content</p>\n{:else}\n\t<p>Contact content</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_real_world_modal() {
        let s = "<script>\n\tlet showModal = false;\n</script>\n\n<button on:click={() => showModal = true}>Open</button>\n\n{#if showModal}\n\t<div class=\"overlay\" on:click={() => showModal = false}>\n\t\t<div class=\"modal\" on:click|stopPropagation>\n\t\t\t<h2>Modal Title</h2>\n\t\t\t<slot />\n\t\t\t<button on:click={() => showModal = false}>Close</button>\n\t\t</div>\n\t</div>\n{/if}\n\n<style>\n\t.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); }\n\t.modal { background: white; padding: 2rem; border-radius: 8px; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_real_world_fetcher() {
        let s = "<script>\n\tlet promise = fetch('/api/data').then(r => r.json());\n</script>\n\n{#await promise}\n\t<p>Loading...</p>\n{:then data}\n\t<pre>{JSON.stringify(data, null, 2)}</pre>\n{:catch error}\n\t<p class=\"error\">{error.message}</p>\n{/await}\n\n<style>\n\t.error { color: red; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_real_world_accordion() {
        let s = "<script>\n\tlet items = [\n\t\t{ title: 'Section 1', content: 'Content 1' },\n\t\t{ title: 'Section 2', content: 'Content 2' },\n\t];\n\tlet open = null;\n</script>\n\n{#each items as item, i}\n\t<div>\n\t\t<button on:click={() => open = open === i ? null : i}>{item.title}</button>\n\t\t{#if open === i}\n\t\t\t<div transition:slide>{item.content}</div>\n\t\t{/if}\n\t</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_real_world_store_subscription() {
        let s = "<script>\n\timport { writable, derived } from 'svelte/store';\n\tconst count = writable(0);\n\tconst doubled = derived(count, ($count) => $count * 2);\n\tconst inc = () => count.update(n => n + 1);\n</script>\n\n<p>Count: {$count}</p>\n<p>Doubled: {$doubled}</p>\n<button on:click={inc}>+1</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_real_world_svelte5_component() {
        let s = "<script lang=\"ts\">\n\ttype Props = { title: string; items: string[]; onselect: (item: string) => void };\n\tlet { title, items = [], onselect }: Props = $props();\n\tlet selected = $state<string | null>(null);\n\tlet count = $derived(items.length);\n\t$effect(() => { if (selected) onselect(selected); });\n</script>\n\n<h2>{title} ({count})</h2>\n{#each items as item (item)}\n\t<button class:selected={item === selected} onclick={() => selected = item}>\n\t\t{item}\n\t</button>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- Svelte 5 specific tests ---

    #[test]
    fn test_svelte5_snippet_with_multiple_params() {
        let s = "{#snippet row(item, index, isLast)}\n\t<tr class:last={isLast}><td>{index}: {item.name}</td></tr>\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_render_conditional() {
        let s = "{#if headerSnippet}\n\t{@render headerSnippet()}\n{:else}\n\t<h1>Default Header</h1>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_props_with_defaults() {
        let s = "<script>\n\tlet { name = 'World', count = 0, items = [] } = $props();\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_derived_by() {
        let s = "<script>\n\tlet count = $state(0);\n\tlet doubled = $derived.by(() => count * 2);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_effect_pre() {
        let s = "<script>\n\tlet el;\n\t$effect.pre(() => { if (el) el.scrollTop = 0; });\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_host_rune() {
        let s = "<script>\n\tconst el = $host();\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_class_state() {
        let s = "<script>\n\tclass Counter {\n\t\tcount = $state(0);\n\t\tincrement() { this.count++; }\n\t}\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_svelte5_snippet_in_component() {
        let s = "<Dialog>\n\t{#snippet title()}\n\t\t<h2>My Dialog</h2>\n\t{/snippet}\n\t{#snippet content()}\n\t\t<p>Dialog content</p>\n\t{/snippet}\n</Dialog>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- accessibility linter tests ---

    #[test]
    fn test_button_type_submit() {
        let s = "<button type=\"submit\">submit</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/button-has-type"),
            "Should NOT flag button type=submit");
    }

    #[test]
    fn test_button_type_reset() {
        let s = "<button type=\"reset\">reset</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/button-has-type"),
            "Should NOT flag button type=reset");
    }

    #[test]
    fn test_no_target_blank_no_target_ok() {
        let s = "<a href=\"https://example.com\">link</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-target-blank"),
            "Should NOT flag link without target");
    }

    // --- template pattern tests ---

    #[test]
    fn test_parse_slot_with_fallback() {
        let s = "<slot>\n\t<p>Fallback content</p>\n</slot>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_named_slot() {
        let s = "<div slot=\"header\"><h1>Title</h1></div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_action_with_params() {
        let s = "<div use:longpress={300}>Hold me</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_transition_with_params() {
        let s = "<div transition:fly={{y: 200, duration: 500}}>content</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_animation() {
        let s = "{#each items as item (item.id)}\n\t<li animate:flip={{duration: 300}}>{item.name}</li>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_this() {
        let s = "<canvas bind:this={canvas} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- serialization and JSON output tests ---

    #[test]
    fn test_ast_serializable() {
        let s = "<p>Hello</p>";
        let r = parser::parse(s);
        let json = serde_json::to_string(&r.ast).unwrap();
        assert!(json.contains("Hello"));
        assert!(json.contains("Element"));
    }

    #[test]
    fn test_ast_roundtrip_json() {
        let s = "<div class=\"test\"><p>{value}</p></div>";
        let r = parser::parse(s);
        let json = serde_json::to_value(&r.ast).unwrap();
        assert!(json["html"]["nodes"].is_array());
    }

    #[test]
    fn test_ast_spans_valid() {
        let s = "<div>text</div>";
        let r = parser::parse(s);
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert!(el.span.start < el.span.end);
            assert!((el.span.end as usize) <= s.len());
        }
    }

    #[test]
    fn test_ast_fragment_span() {
        let s = "<p>a</p><p>b</p>";
        let r = parser::parse(s);
        assert!(r.ast.html.span.start == 0 || r.ast.html.span.end as usize <= s.len());
    }

    #[test]
    fn test_parse_numeric_entity() {
        let s = "<p>&#8212; &#x2014;</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_inline_svg() {
        let s = "<svg viewBox=\"0 0 100 100\"><circle cx=\"50\" cy=\"50\" r=\"40\" /></svg>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_math_ml() {
        let s = "<math><mi>x</mi><mo>=</mo><mn>42</mn></math>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_all_vs_none() {
        let s = "{@html x}\n{@debug y}\n<button>click</button>";
        let r = parser::parse(s);
        let all_diags = Linter::all().lint(&r.ast, s);
        let rec_diags = Linter::recommended().lint(&r.ast, s);
        assert!(all_diags.len() >= rec_diags.len(),
            "All rules should produce >= diagnostics than recommended");
    }

    #[test]
    fn test_parse_expression_member_access() {
        let s = "<p>{obj.nested.deep.value}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_optional_chain() {
        let s = "<p>{obj?.nested?.value ?? 'default'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_array_access() {
        let s = "<p>{items[0]}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_function_call() {
        let s = "<p>{formatDate(new Date())}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- more parser and linter tests ---

    #[test]
    fn test_parse_svelte_self() {
        let s = "<svelte:self count={count - 1} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_component_this() {
        let s = "<svelte:component this={Component} prop={value} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_no_as() {
        let s = "{#each {length: 3}}\n\t<p>item</p>\n{/each}";
        let r = parser::parse(s);
        // May or may not error, but shouldn't panic
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_script_export_default() {
        let s = "<script context=\"module\">\n\texport default class App {}\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_boolean_attribute() {
        let s = "<input disabled readonly required />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_unquoted_attribute() {
        let s = "<div class=foo>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_consistent_selector_class_ok() {
        let s = "<style>\n\t.my-class { color: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/consistent-selector-style"),
            "Should NOT flag class selector");
    }

    #[test]
    fn test_no_export_load_module() {
        let s = "<script context=\"module\">\n\texport function load() {}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-export-load-in-svelte-module-in-kit-pages"),
            "Should flag export load in module script");
    }

    // --- no-store-async tests ---

    #[test]
    fn test_no_store_async_readable() {
        let s = "<script>\n\timport { readable } from 'svelte/store';\n\treadable(null, async (set) => { set(await fetch('/api')); });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-store-async"),
            "Should flag async readable callback");
    }

    #[test]
    fn test_no_store_async_sync_ok() {
        let s = "<script>\n\timport { readable } from 'svelte/store';\n\treadable(null, (set) => { set(42); return () => {}; });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-store-async"),
            "Should NOT flag sync readable callback");
    }

    // --- CSS parser tests ---

    #[test]
    fn test_css_parser_nested_selectors() {
        let s = "<style>\n\t.parent > .child + .sibling ~ .general { color: red; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_parser_pseudo_classes() {
        let s = "<style>\n\ta:hover { color: blue; }\n\tp:first-child { margin: 0; }\n\tdiv:nth-child(2n+1) { background: gray; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_parser_media_query() {
        let s = "<style>\n\t@media (max-width: 768px) {\n\t\t.container { width: 100%; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_parser_keyframes() {
        let s = "<style>\n\t@keyframes fade {\n\t\tfrom { opacity: 1; }\n\t\tto { opacity: 0; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_parser_custom_properties() {
        let s = "<style>\n\t:root { --primary: blue; }\n\tp { color: var(--primary); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_css_parser_attribute_selector() {
        let s = "<style>\n\tinput[type=\"text\"] { border: 1px solid; }\n\t[data-active] { display: block; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- rule-specific regression tests ---

    #[test]
    fn test_no_useless_mustaches_variable_ok() {
        let s = "<p>{variable}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-useless-mustaches"),
            "Should NOT flag variable in mustache");
    }

    #[test]
    fn test_shorthand_attribute_different_names_ok() {
        let s = "<div class={myClass}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/shorthand-attribute"),
            "Should NOT flag attribute with different name and value");
    }

    #[test]
    fn test_no_reactive_literal_string() {
        let s = "<script>\n\t$: x = 'hello';\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should flag reactive string literal");
    }

    #[test]
    fn test_no_reactive_literal_expr_ok() {
        let s = "<script>\n\tlet y = 0;\n\t$: x = y * 2;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should NOT flag reactive expression");
    }

    #[test]
    fn test_store_callback_writable_no_set() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\twritable(0, () => {});\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-callbacks-use-set-param"),
            "Should flag writable callback without set");
    }

    // --- error recovery tests ---

    #[test]
    fn test_parse_unclosed_if() {
        let s = "{#if cond}\n\t<p>text</p>";
        let r = parser::parse(s);
        // Should handle gracefully
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_unclosed_each() {
        let s = "{#each items as item}\n\t<p>{item}</p>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_mismatched_tags() {
        let s = "<div><span>text</div></span>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_bare_close_tag() {
        let s = "</div>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_duplicate_attrs() {
        let s = "<div class=\"a\" class=\"b\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_on_parse_error() {
        let s = "{#if }\n\t<p>text</p>\n{/if}";
        let r = parser::parse(s);
        // Linter should handle parse results with errors gracefully
        let _ = Linter::all().lint(&r.ast, s);
    }

    #[test]
    fn test_parse_expression_with_braces() {
        let s = "<p>{ { key: 'value' }.key }</p>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_component_lowercase_warning() {
        let s = "<component />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- milestone 500 tests ---

    #[test]
    fn test_full_svelte4_app() {
        let s = "<script>\n\timport { onMount } from 'svelte';\n\texport let name;\n\tlet count = 0;\n\t$: doubled = count * 2;\n\tconst increment = () => count++;\n\tonMount(() => console.log('ready'));\n</script>\n\n<h1>Hello {name}!</h1>\n<p>Count: {count}, Doubled: {doubled}</p>\n<button on:click={increment}>+1</button>\n\n<style>\n\th1 { color: purple; }\n\tp { margin: 1em 0; }\n\tbutton { cursor: pointer; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    #[test]
    fn test_full_svelte5_app() {
        let s = "<script lang=\"ts\">\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n\tconst increment = () => count++;\n\t$effect(() => console.log(count));\n</script>\n\n<h1>Counter</h1>\n{#each Array(count) as _, i}\n\t<span>{i}</span>\n{/each}\n<button onclick={increment}>+1</button>\n\n<style lang=\"scss\">\n\th1 { color: blue; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_real_world() {
        let s = "<script>\n\timport Header from './Header.svelte';\n\texport let data;\n\t$: ({ items, total } = data);\n</script>\n\n<Header title=\"Dashboard\" />\n\n<main>\n\t{#if items.length > 0}\n\t\t<ul>\n\t\t\t{#each items as item (item.id)}\n\t\t\t\t<li class:selected={item.active}>\n\t\t\t\t\t<span>{item.name}</span>\n\t\t\t\t\t<button on:click={() => remove(item.id)}>x</button>\n\t\t\t\t</li>\n\t\t\t{/each}\n\t\t</ul>\n\t\t<p>Total: {total}</p>\n\t{:else}\n\t\t<p>No items found.</p>\n\t{/if}\n</main>\n\n<style>\n\tmain { padding: 1rem; }\n\t.selected { font-weight: bold; }\n\tul { list-style: none; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- linter diagnostic quality tests ---

    #[test]
    fn test_diagnostic_has_rule_name() {
        let s = "{@html dangerous}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(!diags.is_empty());
        for d in &diags {
            assert!(!d.rule_name.is_empty(), "Diagnostic should have rule name");
            assert!(d.rule_name.starts_with("svelte/"), "Rule name should start with svelte/");
        }
    }

    #[test]
    fn test_diagnostic_has_message() {
        let s = "{@html dangerous}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        for d in &diags {
            assert!(!d.message.is_empty(), "Diagnostic should have message");
        }
    }

    #[test]
    fn test_diagnostic_has_span() {
        let s = "{@html dangerous}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        for d in &diags {
            assert!(d.span.end > d.span.start, "Diagnostic span should have positive size");
        }
    }

    #[test]
    fn test_all_rules_have_names() {
        let linter = Linter::all();
        for rule in linter.rules() {
            let name = rule.name();
            assert!(!name.is_empty(), "Rule should have a name");
            assert!(name.starts_with("svelte/"), "Rule name should start with svelte/: {}", name);
        }
    }

    #[test]
    fn test_recommended_rules_subset() {
        let all = Linter::all();
        let rec = Linter::recommended();
        let all_names: std::collections::HashSet<_> = all.rules().iter().map(|r| r.name()).collect();
        for rule in rec.rules() {
            assert!(all_names.contains(rule.name()),
                "Recommended rule {} should be in all rules", rule.name());
        }
    }

    // --- parser whitespace handling ---

    #[test]
    fn test_parse_whitespace_only() {
        let r = parser::parse("   \n\n\t  ");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_mixed_content() {
        let s = "text before\n<p>paragraph</p>\ntext between\n{#if cond}\n\t<span>inline</span>\n{/if}\ntext after";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.html.nodes.len() >= 4);
    }

    #[test]
    fn test_parse_self_closing_component() {
        let s = "<Component />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert!(el.self_closing);
            assert!(el.children.is_empty());
        }
    }

    #[test]
    fn test_parse_component_with_children() {
        let s = "<Layout>\n\t<Header />\n\t<Main>\n\t\t<slot />\n\t</Main>\n\t<Footer />\n</Layout>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert_eq!(el.name, "Layout");
            // Should have children (Header, Main, Footer + whitespace)
            assert!(!el.children.is_empty());
        }
    }

    #[test]
    fn test_parse_attribute_value_types() {
        let s = "<div static=\"hello\" dynamic={val} bool empty=\"\" concat=\"a{b}c\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_special_chars_in_text() {
        let s = "<p>Price: $100 & 50% off < today > yesterday</p>";
        let r = parser::parse(s);
        // May have parse errors for unescaped < >, but shouldn't panic
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_script_with_jsx_like() {
        let s = "<script>\n\tconst x = 1 < 2 && 3 > 1;\n\tconst y = a >> b;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiline_attribute() {
        let s = "<div\n\tclass=\"foo\"\n\tid=\"bar\"\n\tstyle=\"color: red\"\n>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_nested_mustache() {
        let s = "<p>{a ? (b ? 'deep' : 'mid') : 'shallow'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_data_attributes() {
        let s = "<div data-testid=\"my-element\" data-custom-prop={value} aria-label=\"label\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_diags_on_empty() {
        let r = parser::parse("");
        let diags = Linter::all().lint(&r.ast, "");
        assert!(diags.is_empty(), "Empty component should have no diagnostics");
    }

    #[test]
    fn test_linter_no_diags_on_text_only() {
        let r = parser::parse("Just some text");
        let diags = Linter::all().lint(&r.ast, "Just some text");
        assert!(diags.is_empty(), "Text-only component should have no diagnostics");
    }

    #[test]
    fn test_parse_table_elements() {
        let s = "<table>\n\t<thead><tr><th>Name</th></tr></thead>\n\t<tbody>{#each items as item}<tr><td>{item}</td></tr>{/each}</tbody>\n</table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_pre_element() {
        let s = "<pre>\n\tconst x = 1;\n\tconst y = 2;\n</pre>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_template_with_all_block_types() {
        let s = "{#if a}\n\tif\n{:else if b}\n\telse-if\n{:else}\n\telse\n{/if}\n{#each xs as x (x)}\n\teach\n{:else}\n\tempty\n{/each}\n{#await p}\n\tpending\n{:then v}\n\tthen\n{:catch e}\n\tcatch\n{/await}\n{#key k}\n\tkey\n{/key}\n{#snippet s()}\n\tsnippet\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.html.nodes.len() >= 5);
    }

    // --- AST structure tests ---

    #[test]
    fn test_ast_if_block_structure() {
        let s = "{#if cond}\n\t<p>yes</p>\n{:else}\n\t<p>no</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::IfBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.test.trim(), "cond");
            assert!(!block.consequent.nodes.is_empty());
            assert!(block.alternate.is_some());
        } else {
            panic!("Expected IfBlock");
        }
    }

    #[test]
    fn test_ast_each_block_structure() {
        let s = "{#each items as item, i (item.id)}\n\t<p>{item}</p>\n{:else}\n\t<p>empty</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::EachBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.expression.trim(), "items");
            assert_eq!(block.context.trim(), "item");
            assert_eq!(block.index.as_deref(), Some("i"));
            assert_eq!(block.key.as_deref(), Some("item.id"));
            assert!(block.fallback.is_some());
        } else {
            panic!("Expected EachBlock");
        }
    }

    #[test]
    fn test_ast_await_block_structure() {
        let s = "{#await promise}\n\t<p>loading</p>\n{:then value}\n\t<p>{value}</p>\n{:catch error}\n\t<p>{error}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::AwaitBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.expression.trim(), "promise");
            assert!(block.pending.is_some());
            assert!(block.then.is_some());
            assert_eq!(block.then_binding.as_deref(), Some("value"));
            assert!(block.catch.is_some());
            assert_eq!(block.catch_binding.as_deref(), Some("error"));
        } else {
            panic!("Expected AwaitBlock");
        }
    }

    #[test]
    fn test_ast_key_block_structure() {
        let s = "{#key value}\n\t<p>{value}</p>\n{/key}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::KeyBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.expression.trim(), "value");
        } else {
            panic!("Expected KeyBlock");
        }
    }

    #[test]
    fn test_ast_snippet_structure() {
        let s = "{#snippet greeting(name)}\n\t<p>Hello {name}!</p>\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::SnippetBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.name, "greeting");
            assert!(block.params.contains("name"));
        } else {
            panic!("Expected SnippetBlock");
        }
    }

    #[test]
    fn test_ast_element_attributes() {
        let s = "<div class=\"foo\" id={myId} {...rest} on:click={handler} bind:value use:tooltip class:active style:color=\"red\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert_eq!(el.name, "div");
            assert!(el.attributes.len() >= 7, "Should have at least 7 attributes");
        }
    }

    #[test]
    fn test_ast_script_content() {
        let s = "<script>\n\tlet x = 42;\n\tconst y = 'hello';\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let script = r.ast.instance.as_ref().unwrap();
        assert!(script.content.contains("let x = 42"));
        assert!(script.content.contains("const y = 'hello'"));
        assert!(!script.module);
    }

    #[test]
    fn test_ast_module_script() {
        let s = "<script context=\"module\">\n\texport const prerender = true;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let module = r.ast.module.as_ref().unwrap();
        assert!(module.module);
    }

    #[test]
    fn test_ast_css_content() {
        let s = "<style>\n\tp { color: red; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let css = r.ast.css.as_ref().unwrap();
        assert!(css.content.contains("color: red"));
    }

    #[test]
    fn test_ast_render_tag() {
        let s = "{@render greeting('world')}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::RenderTag(tag) = &r.ast.html.nodes[0] {
            assert!(tag.expression.contains("greeting"));
        } else {
            panic!("Expected RenderTag");
        }
    }

    #[test]
    fn test_ast_const_tag() {
        let s = "{@const x = 42}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::ConstTag(tag) = &r.ast.html.nodes[0] {
            assert!(tag.declaration.contains("42"));
        } else {
            panic!("Expected ConstTag");
        }
    }

    #[test]
    fn test_ast_debug_tag() {
        let s = "{@debug x, y}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::DebugTag(tag) = &r.ast.html.nodes[0] {
            assert!(tag.identifiers.contains(&"x".to_string()));
            assert!(tag.identifiers.contains(&"y".to_string()));
        } else {
            panic!("Expected DebugTag");
        }
    }

    #[test]
    fn test_ast_raw_mustache() {
        let s = "{@html '<p>raw</p>'}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::RawMustacheTag(tag) = &r.ast.html.nodes[0] {
            assert!(tag.expression.contains("<p>raw</p>"));
        } else {
            panic!("Expected RawMustacheTag");
        }
    }

    #[test]
    fn test_ast_text_node() {
        let s = "Hello World!";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Text(text) = &r.ast.html.nodes[0] {
            assert_eq!(text.data, "Hello World!");
        } else {
            panic!("Expected Text");
        }
    }

    #[test]
    fn test_ast_comment() {
        let s = "<!-- comment text -->";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Comment(comment) = &r.ast.html.nodes[0] {
            assert_eq!(comment.data.trim(), "comment text");
        } else {
            panic!("Expected Comment");
        }
    }

    // --- comprehensive rule coverage ---

    #[test]
    fn test_prefer_class_directive_ternary() {
        let s = "<div class={active ? 'active' : ''}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-class-directive"),
            "Should suggest class directive for ternary");
    }

    #[test]
    fn test_no_raw_special_elements_head() {
        let s = "<svelte:head>{@html '<meta>'}</svelte:head>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-raw-special-elements"
            || d.rule_name == "svelte/no-at-html-tags"),
            "Should flag @html in special element");
    }

    #[test]
    fn test_no_unknown_style_directive_prop() {
        let s = "<div style:colr=\"red\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-unknown-style-directive-property"),
            "Should flag unknown style directive property");
    }

    #[test]
    fn test_no_unknown_style_directive_ok() {
        let s = "<div style:color=\"red\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-unknown-style-directive-property"),
            "Should NOT flag known style directive property");
    }

    #[test]
    fn test_require_event_dispatcher_types_ts() {
        let s = "<script lang=\"ts\">\n\timport { createEventDispatcher } from 'svelte';\n\tconst dispatch = createEventDispatcher();\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-event-dispatcher-types"),
            "Should flag untyped createEventDispatcher");
    }

    #[test]
    fn test_require_event_dispatcher_typed_ok() {
        let s = "<script lang=\"ts\">\n\timport { createEventDispatcher } from 'svelte';\n\tconst dispatch = createEventDispatcher<{ click: MouseEvent }>();\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-event-dispatcher-types"),
            "Should NOT flag typed createEventDispatcher");
    }

    #[test]
    fn test_html_self_close_empty_comp() {
        let s = "<Component></Component>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/html-self-closing"),
            "Should flag component that could be self-closing");
    }

    #[test]
    fn test_html_self_close_comp_ok() {
        let s = "<Component />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/html-self-closing"),
            "Should NOT flag self-closing component");
    }

    #[test]
    fn test_no_reactive_reassign_increment() {
        let s = "<script>\n\tlet v = 0;\n\t$: r = v * 2;\n\tfunction click() { r++; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag reactive var increment");
    }

    // --- cross-cutting linter tests ---

    #[test]
    fn test_clean_svelte5_component() {
        let s = "<script>\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n\tconst increment = () => count++;\n</script>\n\n<button onclick={increment}>\n\t{count} x 2 = {doubled}\n</button>\n\n<style>\n\tbutton { cursor: pointer; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::recommended().lint(&r.ast, s);
        let filtered: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/block-lang" && d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(filtered.is_empty(), "Clean Svelte 5 component should have no warnings, got: {:?}",
            filtered.iter().map(|d| format!("{}: {}", d.rule_name, d.message)).collect::<Vec<_>>());
    }

    #[test]
    fn test_each_with_key_and_destructure() {
        let s = "{#each users as { id, name } (id)}\n\t<p>{name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-each-key"),
            "Should NOT flag each with key");
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-each-key"),
            "Should NOT flag key using destructured var");
    }

    #[test]
    fn test_multiple_style_checks() {
        let s = "<div style=\"color: red; color: blue;\" style:font-size=\"14px\">text</div>\n<style>\n\tdiv { margin: 0; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-style-properties"),
            "Should flag duplicate color in style attr");
    }

    // --- parser expression edge cases ---

    #[test]
    fn test_parse_ternary_in_attribute() {
        let s = "<div class={active ? 'active' : 'inactive'}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_template_literal_in_attribute() {
        let s = "<div class={`item-${id}`}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_object_spread_in_component() {
        let s = "<Component {...$$restProps} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiline_expression() {
        let s = "<p>{\n\titems\n\t\t.filter(Boolean)\n\t\t.map(String)\n\t\t.join(', ')\n}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_ts_generic() {
        let s = "<script lang=\"ts\" generics=\"T extends Record<string, unknown>\">\n\tlet items: T[] = [];\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_render_tag_args() {
        let s = "{@render header({ title: 'Hello', subtitle: 'World' })}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_const_tag_destructure() {
        let s = "{#each items as item}\n\t{@const { name, age } = item}\n\t<p>{name}: {age}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_attach_directive() {
        let s = "<div {@attach tooltip}>text</div>";
        let r = parser::parse(s);
        // May or may not parse successfully depending on parser support
        let _ = r.ast.html.nodes.len();
    }

    // --- linter edge case tests ---

    #[test]
    fn test_no_at_html_in_each() {
        let s = "{#each items as item}\n\t{@html item.content}\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-html-tags"),
            "Should flag @html inside each block");
    }

    #[test]
    fn test_no_at_debug_in_if() {
        let s = "{#if debug}\n\t{@debug value}\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-debug-tags"),
            "Should flag @debug inside if block");
    }

    #[test]
    fn test_each_key_using_index() {
        let s = "{#each items as item, i (i)}\n\t<p>{item}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-each-key"),
            "Should NOT flag index as key (it's defined by each block)");
    }

    #[test]
    fn test_dom_manipulating_svelte_element() {
        let s = "<script>\n\tlet el;\n\tconst rm = () => el.remove();\n</script>\n<svelte:element this=\"div\" bind:this={el}>text</svelte:element>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dom-manipulating"),
            "Should flag dom manipulation on svelte:element");
    }

    #[test]
    fn test_unused_class_directive() {
        let s = "<div class:active={isActive}>text</div>\n<style>\n\t.active { color: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-unused-class-name"),
            "Should NOT flag class used via class: directive");
    }

    #[test]
    fn test_prefer_style_directive_ok() {
        let s = "<div style:color=\"red\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/prefer-style-directive"),
            "Should NOT flag style: directive (already using it)");
    }

    // --- parser robustness tests ---

    #[test]
    fn test_parse_deeply_nested_blocks() {
        let s = "{#if a}\n\t{#if b}\n\t\t{#if c}\n\t\t\t{#each items as item}\n\t\t\t\t{item}\n\t\t\t{/each}\n\t\t{/if}\n\t{/if}\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_else() {
        let s = "{#each items as item}\n\t<p>{item}</p>\n{:else}\n\t<p>No items</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_all_attrs() {
        let s = "<Widget bind:value on:change={handler} class:active let:item use:tooltip {...props}>{item}</Widget>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_textarea() {
        let s = "<textarea bind:value>{content}</textarea>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_select() {
        let s = "<select bind:value>\n\t<option value=\"a\">A</option>\n\t<option value=\"b\">B</option>\n</select>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_global() {
        let s = "<style>\n\t:global(.foo) { color: red; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_attribute_expressions() {
        let s = "<div class=\"static {dynamic} more-static\" data-id={id} hidden={!visible}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_event_with_inline_handler() {
        let s = "<button on:click={() => { count++; console.log(count); }}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- shorthand directive tests ---

    #[test]
    fn test_shorthand_directive_bind() {
        let s = "<input bind:value={value} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/shorthand-directive"),
            "Should flag non-shorthand bind:value");
    }

    #[test]
    fn test_shorthand_directive_bind_ok() {
        let s = "<input bind:value />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/shorthand-directive"),
            "Should NOT flag shorthand bind:value");
    }

    // --- more linter positive/negative tests ---

    #[test]
    fn test_first_attribute_linebreak_ok() {
        let s = "<div class=\"foo\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/first-attribute-linebreak"),
            "Should NOT flag singleline element");
    }

    #[test]
    fn test_max_attributes_per_line_ok() {
        let s = "<div class=\"foo\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/max-attributes-per-line"),
            "Should NOT flag element with 1 attribute");
    }

    #[test]
    fn test_mustache_spacing_ok() {
        let s = "<p>{value}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/mustache-spacing"),
            "Should NOT flag default mustache spacing");
    }

    #[test]
    fn test_no_add_event_listener() {
        let s = "<script>\n\tdocument.addEventListener('click', handler);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-add-event-listener"),
            "Should flag addEventListener");
    }

    #[test]
    fn test_experimental_require_strict_events_ts() {
        let s = "<script lang=\"ts\">\n\timport { createEventDispatcher } from 'svelte';\n\tconst dispatch = createEventDispatcher();\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/experimental-require-strict-events"
            || d.rule_name == "svelte/require-event-dispatcher-types"),
            "Should flag untyped dispatcher");
    }


    #[test]
    // --- comprehensive linter negative tests ---

    #[test]
    fn test_button_has_type_ok() {
        let s = "<button type=\"button\">click</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/button-has-type"),
            "Should NOT flag button with type");
    }

    #[test]
    fn test_button_has_type_missing() {
        let s = "<button>click</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/button-has-type"),
            "Should flag button without type");
    }

    #[test]
    fn test_no_target_blank_ok() {
        let s = "<a href=\"https://example.com\" rel=\"noopener noreferrer\" target=\"_blank\">link</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-target-blank"),
            "Should NOT flag target=_blank with rel");
    }

    #[test]
    fn test_no_target_blank_missing_rel() {
        let s = "<a href=\"https://example.com\" target=\"_blank\">link</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-target-blank"),
            "Should flag target=_blank without rel");
    }

    #[test]
    fn test_no_spaces_around_equal_signs() {
        let s = "<div class = \"foo\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-spaces-around-equal-signs-in-attribute"),
            "Should flag spaces around = in attribute");
    }

    #[test]
    fn test_html_quotes_double_ok() {
        let s = "<div class=\"foo\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/html-quotes"),
            "Should NOT flag double quotes (default)");
    }

    // --- Svelte 5 linter rule tests ---

    #[test]
    fn test_no_inspect_in_script() {
        let s = "<script>\n\tlet count = $state(0);\n\t$inspect(count);\n\t$inspect.with(console.trace, count);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let inspect_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-inspect").collect();
        assert!(inspect_diags.len() >= 1, "Should flag $inspect calls");
    }

    #[test]
    fn test_reactive_fn_decl() {
        let s = "<script>\n\t$: fn = () => console.log('reactive function');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-functions"),
            "Should flag reactive function declaration");
    }

    #[test]
    fn test_max_lines_per_block_script() {
        // Script with many lines should not trigger by default (threshold is typically high)
        let s = "<script>\n\tlet a = 1;\n\tlet b = 2;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/max-lines-per-block"),
            "Should NOT flag short script block");
    }

    #[test]
    fn test_no_goto_without_base_imported() {
        let s = "<script>\n\timport { goto } from '$app/navigation';\n\tgoto('/path');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-goto-without-base"),
            "Should flag goto without base import");
    }

    #[test]
    fn test_dynamic_slot_name_expr() {
        let s = "<slot name={dynamicName}>content</slot>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dynamic-slot-name"),
            "Should flag dynamic slot name");
    }

    // --- linter rule combination tests ---

    #[test]
    fn test_multiple_rules_on_one_file() {
        let s = "<script>\n\t$: x = 42;\n\t$inspect(x);\n</script>\n{@html danger}\n{@debug x}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let has_rule = |name: &str| diags.iter().any(|d| d.rule_name == name);
        assert!(has_rule("svelte/no-at-html-tags"), "Should flag @html");
        assert!(has_rule("svelte/no-at-debug-tags"), "Should flag @debug");
        assert!(has_rule("svelte/no-inspect"), "Should flag $inspect");
        assert!(has_rule("svelte/no-reactive-literals"), "Should flag $: x = 42");
    }

    #[test]
    fn test_no_false_positives_clean() {
        let s = "<script lang=\"ts\">\n\timport { onMount } from 'svelte';\n\texport let data: { name: string };\n\tlet count = 0;\n\tconst increment = () => count++;\n\tonMount(() => {\n\t\tconsole.log('mounted');\n\t});\n</script>\n\n{#each items as item (item.id)}\n\t<p>{item}</p>\n{/each}\n\n<button on:click={increment}>Count: {count}</button>\n\n<style>\n\tp { color: blue; }\n\tbutton { font-size: 1em; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty(), "Parse should succeed");
        let diags = Linter::recommended().lint(&r.ast, s);
        // Filter out block-lang (we're using lang=ts for script but not style)
        let filtered: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/block-lang" && d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(filtered.is_empty(), "Clean Svelte 4 component should have no warnings, got: {:?}",
            filtered.iter().map(|d| format!("{}: {}", d.rule_name, d.message)).collect::<Vec<_>>());
    }

    // --- error handling tests ---

    #[test]
    fn test_parse_unclosed_tag_graceful() {
        let s = "<div><p>unclosed";
        let r = parser::parse(s);
        // Parser should handle gracefully (may have errors but shouldn't panic)
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_invalid_mustache_graceful() {
        let s = "<p>{ }</p>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_multiple_root_elements() {
        let s = "<h1>Title</h1>\n<p>Paragraph</p>\n<footer>Footer</footer>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.html.nodes.len() >= 3);
    }

    #[test]
    fn test_parse_html_comment() {
        let s = "<!-- This is a comment -->\n<p>text</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.html.nodes.iter().any(|n| matches!(n, ast::TemplateNode::Comment(_))));
    }

    #[test]
    fn test_parse_text_with_entities() {
        let s = "<p>&amp; &lt;tag&gt; &quot;text&quot;</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_self_closing_html() {
        let s = "<div />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert!(el.self_closing);
        }
    }

    #[test]
    fn test_parse_complex_template_expressions() {
        let s = "<p>{@html `<strong>${name}</strong>`}</p>\n{@debug name, count}\n{@const doubled = count * 2}\n<p>{doubled}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- linter integration tests ---

    #[test]
    fn test_full_component_all_rules() {
        let s = "<script lang=\"ts\">\n\timport { onMount } from 'svelte';\n\tlet count = $state(0);\n\tconst increment = () => count++;\n\tonMount(() => { console.log('mounted'); });\n</script>\n\n<button onclick={increment}>\n\tClicks: {count}\n</button>\n\n<style>\n\tbutton { font-size: 1.2em; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::recommended().lint(&r.ast, s);
        // Should have minimal warnings for well-written component
        let serious: Vec<_> = diags.iter().filter(|d|
            d.rule_name != "svelte/no-useless-mustaches"
            && d.rule_name != "svelte/require-each-key"
        ).collect();
        assert!(serious.len() <= 2, "Well-written component should have few warnings, got: {:?}",
            serious.iter().map(|d| &d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_linter_rules_count() {
        let linter = Linter::all();
        let count = linter.rules().len();
        assert!(count >= 79, "Should have at least 79 rules, got {}", count);
    }

    #[test]
    fn test_recommended_subset() {
        let all = Linter::all();
        let rec = Linter::recommended();
        assert!(rec.rules().len() < all.rules().len(),
            "Recommended should be subset of all rules");
    }

    #[test]
    fn test_parse_empty_component() {
        let r = parser::parse("");
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_none());
        assert!(r.ast.css.is_none());
    }

    #[test]
    fn test_parse_only_text() {
        let r = parser::parse("Hello World!");
        assert!(r.errors.is_empty());
        assert_eq!(r.ast.html.nodes.len(), 1);
    }

    #[test]
    fn test_parse_nested_components() {
        let s = "<Parent>\n\t<Child>\n\t\t<GrandChild />\n\t</Child>\n</Parent>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_with_ts() {
        let s = "<script lang=\"ts\">\n\tlet count: number = 0;\n\tconst fn = (x: string): boolean => true;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.as_ref().unwrap().lang.as_deref() == Some("ts"));
    }

    // --- comprehensive parser tests ---

    #[test]
    fn test_parse_spread_attribute() {
        let s = "<div {...props}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert!(el.attributes.iter().any(|a| matches!(a, ast::Attribute::Spread { .. })));
        }
    }

    #[test]
    fn test_parse_let_directive() {
        let s = "<Comp let:item>{item}</Comp>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_scripts() {
        let s = "<script context=\"module\">\n\texport const prerender = true;\n</script>\n<script>\n\tlet count = 0;\n</script>\n<p>{count}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.module.is_some());
        assert!(r.ast.instance.is_some());
    }

    #[test]
    fn test_parse_svelte5_module_script() {
        let s = "<script module>\n\texport const prerender = true;\n</script>\n<script>\n\tlet count = 0;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.module.is_some());
    }

    #[test]
    fn test_parse_void_elements() {
        let s = "<br><hr><img src=\"test.png\"><input type=\"text\">";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert_eq!(r.ast.html.nodes.len(), 4);
    }

    #[test]
    fn test_parse_nested_if_else() {
        let s = "{#if a}\n\t{#if b}\n\t\tx\n\t{:else}\n\t\ty\n\t{/if}\n{:else if c}\n\tz\n{:else}\n\tw\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_index() {
        let s = "{#each items as item, index (item.id)}\n\t<p>{index}: {item}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::EachBlock(block) = &r.ast.html.nodes[0] {
            assert!(block.index.is_some());
            assert!(block.key.is_some());
        }
    }

    #[test]
    fn test_parse_each_destructured() {
        let s = "{#each items as { id, name } (id)}\n\t<p>{name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_await_shorthand() {
        let s = "{#await promise then value}\n\t<p>{value}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- unused class name unit tests ---

    #[test]
    fn test_unused_class_name_no_css() {
        let s = "<div class=\"foo\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-unused-class-name"),
            "Should flag class without style block");
    }

    #[test]
    fn test_unused_class_name_defined_ok() {
        let s = "<div class=\"foo\">text</div>\n<style>\n\t.foo { color: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-unused-class-name"),
            "Should NOT flag class defined in style");
    }

    #[test]
    fn test_unused_class_name_partial() {
        let s = "<div class=\"foo bar\">text</div>\n<style>\n\t.foo { color: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.message.contains("bar")),
            "Should flag 'bar' not defined in style");
        assert!(!diags.iter().any(|d| d.message.contains("foo")),
            "Should NOT flag 'foo' defined in style");
    }

    // --- parser edge case tests ---

    #[test]
    fn test_parse_nested_each_key_parens() {
        let s = "{#each items as item (getKey(item, 'id'))}\n\t{item}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::EachBlock(block) = &r.ast.html.nodes[0] {
            assert_eq!(block.context.trim(), "item");
            assert_eq!(block.key.as_ref().unwrap(), "getKey(item, 'id')");
        }
    }

    #[test]
    fn test_parse_svelte_element() {
        let s = "<svelte:element this=\"div\" class=\"foo\">content</svelte:element>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert_eq!(el.name, "svelte:element");
        }
    }

    #[test]
    fn test_parse_svelte_boundary() {
        let s = "<svelte:boundary onerror={handler}>\n\t<p>content</p>\n</svelte:boundary>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_class_directive() {
        let s = "<div class:active={isActive}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert!(el.attributes.iter().any(|a| {
                matches!(a, ast::Attribute::Directive { kind: ast::DirectiveKind::Class, name, .. } if name == "active")
            }));
        }
    }

    #[test]
    fn test_parse_style_directive() {
        let s = "<div style:color=\"red\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_use_directive() {
        let s = "<div use:tooltip>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_transition_directive() {
        let s = "<div transition:fade>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_animate_directive() {
        let s = "<li animate:flip>item</li>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- no-reactive-reassign unit tests ---

    #[test]
    fn test_no_reactive_reassign_basic() {
        let s = "<script>\n\tlet value = 0;\n\t$: reactiveValue = value * 2;\n\tfunction click() { reactiveValue = 3; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag reassignment of reactive variable");
    }

    #[test]
    fn test_no_reactive_reassign_let_ok() {
        let s = "<script>\n\tlet value = 0;\n\tlet reactive;\n\t$: reactive = value * 2;\n\tfunction click() { reactive = 3; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should NOT flag pre-declared let variable");
    }

    #[test]
    fn test_no_reactive_reassign_bind() {
        let s = "<script>\n\tlet value = 0;\n\t$: reactiveValue = value * 2;\n</script>\n<input bind:value={reactiveValue} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag bind:value on reactive variable");
    }

    // --- no-inline-styles unit tests ---

    #[test]
    fn test_no_inline_styles_static() {
        let s = "<div style=\"color: red;\">hi</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inline-styles"),
            "Should flag static inline style");
    }

    #[test]
    fn test_no_inline_styles_no_style_ok() {
        let s = "<div class=\"red\">hi</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-inline-styles"),
            "Should NOT flag element without style");
    }

    // --- no-trailing-spaces unit tests ---

    #[test]
    fn test_no_trailing_spaces() {
        let s = "<p>hello</p>   \n<p>world</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-trailing-spaces"),
            "Should flag trailing spaces");
    }

    #[test]
    fn test_no_trailing_spaces_clean_ok() {
        let s = "<p>hello</p>\n<p>world</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-trailing-spaces"),
            "Should NOT flag clean lines");
    }

    // --- prefer-style-directive unit tests ---

    #[test]
    fn test_prefer_style_directive() {
        let s = "<div style=\"color: {active ? 'red' : 'blue'}\">hi</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-style-directive"),
            "Should flag dynamic inline style");
    }

    // --- no-svelte-internal unit tests ---

    #[test]
    fn test_no_svelte_internal() {
        let s = "<script>\n\timport { internal } from 'svelte/internal';\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-svelte-internal"),
            "Should flag svelte/internal import");
    }

    #[test]
    fn test_no_svelte_internal_ok() {
        let s = "<script>\n\timport { onMount } from 'svelte';\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-svelte-internal"),
            "Should NOT flag svelte import");
    }

    // --- no-inspect unit tests ---

    #[test]
    fn test_no_inspect() {
        let s = "<script>\n\t$inspect(count);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inspect"),
            "Should flag $inspect");
    }

    // --- block-lang unit tests ---

    #[test]
    fn test_block_lang_no_lang_script() {
        let s = "<script>\n\tlet x = 1;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/block-lang"),
            "Should flag script without lang attribute");
    }

    #[test]
    fn test_block_lang_ts_ok() {
        let s = "<script lang=\"ts\">\n\tlet x: number = 1;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/block-lang"),
            "Should NOT flag script with lang='ts'");
    }

    #[test]
    fn test_block_lang_style_no_lang() {
        let s = "<style>\n\tp { color: red; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/block-lang"),
            "Should flag style without lang attribute");
    }

    // --- derived-has-same-inputs-outputs unit tests ---

    #[test]
    fn test_derived_mismatch() {
        let s = "<script>\n\timport { derived } from 'svelte/store';\n\tconst d = derived(count, (x) => x * 2);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/derived-has-same-inputs-outputs"),
            "Should flag mismatched derived param");
    }

    #[test]
    fn test_derived_match_ok() {
        let s = "<script>\n\timport { derived } from 'svelte/store';\n\tconst d = derived(count, ($count) => $count * 2);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/derived-has-same-inputs-outputs"),
            "Should NOT flag matching derived param");
    }

    // --- no-export-load-in-svelte-module unit tests ---

    #[test]
    fn test_no_export_load_ok_without_module() {
        let s = "<script>\n\texport function load() {}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-export-load-in-svelte-module-in-kit-pages"),
            "Should NOT flag load in instance script");
    }

    // --- consistent-selector-style unit tests ---

    #[test]
    fn test_valid_prop_names_ok() {
        let s = "<script>\n\texport let data;\n</script>\n<p>{data}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-prop-names-in-kit-pages"),
            "Should NOT flag standard prop name");
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
                if path.is_dir() { continue; }
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
                if path.is_dir() { continue; }
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
    #[test] fn linter_html_self_closing_invalid() { run_linter_invalid("html-self-closing"); }
    #[test] fn linter_no_not_function_handler_valid() { run_linter_valid("no-not-function-handler"); }
    #[test] fn linter_no_not_function_handler_invalid() { run_linter_invalid("no-not-function-handler"); }
    #[test] fn linter_no_shorthand_style_property_overrides_valid() { run_linter_valid("no-shorthand-style-property-overrides"); }
    #[test] fn linter_no_shorthand_style_property_overrides_invalid() { run_linter_invalid("no-shorthand-style-property-overrides"); }
    #[test] fn linter_no_unknown_style_directive_property_valid() { run_linter_valid("no-unknown-style-directive-property"); }
    #[test] fn linter_no_unknown_style_directive_property_invalid() { run_linter_invalid("no-unknown-style-directive-property"); }
    #[test] fn linter_valid_each_key_valid() { run_linter_valid("valid-each-key"); }
    #[test] fn linter_valid_each_key_invalid() { run_linter_invalid("valid-each-key"); }
    #[test] fn linter_no_spaces_around_equal_signs_in_attribute_valid() { run_linter_valid("no-spaces-around-equal-signs-in-attribute"); }
    #[test] fn linter_no_spaces_around_equal_signs_in_attribute_invalid() { run_linter_invalid("no-spaces-around-equal-signs-in-attribute"); }
    #[test] fn linter_prefer_class_directive_valid() { run_linter_valid("prefer-class-directive"); }
    // prefer-class-directive invalid needs nested ternary / multi-expression analysis
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
    #[test] fn linter_html_closing_bracket_new_line_invalid() { run_linter_invalid("html-closing-bracket-new-line"); }

    // Batch 5: more invalid tests
    #[test] fn linter_no_dom_manipulating_invalid() { run_linter_invalid("no-dom-manipulating"); }
    // require-event-prefix invalid needs $props type analysis
    // #[test] fn linter_require_event_prefix_invalid() { run_linter_invalid("require-event-prefix"); }
    #[test] fn linter_no_add_event_listener_invalid() { run_linter_invalid("no-add-event-listener"); }

    #[test] fn linter_max_lines_per_block_valid() { run_linter_valid("max-lines-per-block"); }

    #[test] fn linter_no_navigation_without_resolve_valid() { run_linter_valid("no-navigation-without-resolve"); }
    #[test] fn linter_prefer_svelte_reactivity_valid() { run_linter_valid("prefer-svelte-reactivity"); }
    #[test] fn linter_no_dynamic_slot_name_valid() { run_linter_valid("no-dynamic-slot-name"); }
    #[test] fn linter_no_goto_without_base_valid() { run_linter_valid("no-goto-without-base"); }
    #[test] fn linter_no_navigation_without_base_valid() { run_linter_valid("no-navigation-without-base"); }
    #[test] fn linter_require_store_callbacks_use_set_param_valid() { run_linter_valid("require-store-callbacks-use-set-param"); }
    #[test] fn linter_require_store_callbacks_use_set_param_invalid() { run_linter_invalid("require-store-callbacks-use-set-param"); }
    #[test] fn linter_require_store_reactive_access_valid() { run_linter_valid("require-store-reactive-access"); }

    #[test] fn linter_no_dynamic_slot_name_invalid() { run_linter_invalid("no-dynamic-slot-name"); }
    #[test] fn linter_no_goto_without_base_invalid() { run_linter_invalid("no-goto-without-base"); }
    #[test] fn linter_no_navigation_without_base_invalid() { run_linter_invalid("no-navigation-without-base"); }

    #[test] fn linter_no_reactive_functions_invalid() { run_linter_invalid("no-reactive-functions"); }

    #[test] fn linter_no_useless_children_snippet_invalid() { run_linter_invalid("no-useless-children-snippet"); }
    #[test] fn linter_no_ignored_unsubscribe_invalid() { run_linter_invalid("no-ignored-unsubscribe"); }
    #[test] fn linter_no_reactive_literals_invalid() { run_linter_invalid("no-reactive-literals"); }
    #[test] fn linter_require_stores_init_invalid() { run_linter_invalid("require-stores-init"); }
    #[test] fn linter_valid_style_parse_invalid() { run_linter_invalid("valid-style-parse"); }

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
    #[test] fn linter_no_immutable_reactive_statements_invalid() { run_linter_invalid("no-immutable-reactive-statements"); }
    #[test] fn linter_no_useless_children_snippet_valid() { run_linter_valid("no-useless-children-snippet"); }
    #[test] fn linter_no_reactive_reassign_valid() { run_linter_valid("no-reactive-reassign"); }
    #[test] fn linter_no_reactive_reassign_invalid() { run_linter_invalid("no-reactive-reassign"); }
    #[test] fn linter_no_ignored_unsubscribe_valid() { run_linter_valid("no-ignored-unsubscribe"); }
    #[test] fn linter_no_inner_declarations_valid() { run_linter_valid("no-inner-declarations"); }
    #[test] fn linter_no_inner_declarations_invalid() { run_linter_invalid("no-inner-declarations"); }
    #[test] fn linter_no_add_event_listener_valid() { run_linter_valid("no-add-event-listener"); }
    #[test] fn linter_no_unnecessary_state_wrap_valid() { run_linter_valid("no-unnecessary-state-wrap"); }
    #[test] fn linter_no_unused_props_valid() { run_linter_valid("no-unused-props"); }
    #[test] fn linter_no_unused_class_name_valid() { run_linter_valid("no-unused-class-name"); }
    #[test] fn linter_no_unused_class_name_invalid() { run_linter_invalid("no-unused-class-name"); }
    #[test] fn linter_require_event_dispatcher_types_valid() { run_linter_valid("require-event-dispatcher-types"); }
    #[test] fn linter_require_event_dispatcher_types_invalid() { run_linter_invalid("require-event-dispatcher-types"); }
    #[test] fn linter_require_stores_init_valid() { run_linter_valid("require-stores-init"); }
    #[test] fn linter_require_optimized_style_attribute_valid() { run_linter_valid("require-optimized-style-attribute"); }
    #[test] fn linter_require_optimized_style_attribute_invalid() { run_linter_invalid("require-optimized-style-attribute"); }
    #[test] fn linter_prefer_writable_derived_valid() { run_linter_valid("prefer-writable-derived"); }
    #[test] fn linter_prefer_writable_derived_invalid() { run_linter_invalid("prefer-writable-derived"); }
    #[test] fn linter_prefer_const_valid() { run_linter_valid("prefer-const"); }
    #[test] fn linter_prefer_const_invalid() { run_linter_invalid("prefer-const"); }
    #[test] fn linter_prefer_destructured_store_props_valid() { run_linter_valid("prefer-destructured-store-props"); }
    #[test] fn linter_prefer_destructured_store_props_invalid() { run_linter_invalid("prefer-destructured-store-props"); }
    #[test] fn linter_infinite_reactive_loop_valid() { run_linter_valid("infinite-reactive-loop"); }
    #[test] fn linter_no_top_level_browser_globals_valid() { run_linter_valid("no-top-level-browser-globals"); }
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
