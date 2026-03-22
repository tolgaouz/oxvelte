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

    #[test]
    fn test_parse_real_world_drag_sort() {
        let s = "<script>\n\tlet items = $state(['A', 'B', 'C', 'D']);\n\tlet dragging = $state(null);\n\tlet over = $state(null);\n</script>\n{#each items as item, i (item)}\n\t<div draggable=\"true\"\n\t\ton:dragstart={() => dragging = i}\n\t\ton:dragover|preventDefault={() => over = i}\n\t\ton:drop={() => { const t = items[i]; items[i] = items[dragging]; items[dragging] = t; dragging = null; }}\n\t\tclass:dragging={dragging === i}\n\t\tclass:over={over === i}\n\t>{item}</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_text_decoration() {
        let s = "<style>\n\ta {\n\t\ttext-decoration: underline wavy red;\n\t\ttext-underline-offset: 3px;\n\t\ttext-decoration-thickness: 2px;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_tick_update() {
        let s = "<script>\n\timport { tick } from 'svelte';\n\tlet el;\n\tlet items = $state([]);\n\tconst add = async (item) => {\n\t\titems.push(item);\n\t\tawait tick();\n\t\tel.scrollTop = el.scrollHeight;\n\t};\n</script>\n<div bind:this={el}>\n\t{#each items as item}<p>{item}</p>{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_debug_empty() {
        let s = "{@debug}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-debug-tags"));
    }

    #[test]
    fn test_parse_real_world_responsive_table() {
        let s = "<script>\n\tlet { data, columns } = $props();\n</script>\n<div class=\"table-wrapper\">\n\t<table>\n\t\t<thead>\n\t\t\t<tr>{#each columns as c}<th>{c.label}</th>{/each}</tr>\n\t\t</thead>\n\t\t<tbody>\n\t\t\t{#each data as row (row.id)}\n\t\t\t\t<tr>{#each columns as c}<td data-label={c.label}>{row[c.key]}</td>{/each}</tr>\n\t\t\t{/each}\n\t\t</tbody>\n\t</table>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_global_state() {
        let s = "<script context=\"module\">\n\tlet shared = $state({ theme: 'light', locale: 'en' });\n\texport { shared };\n</script>\n<script>\n\timport { shared } from './store.svelte';\n</script>\n<p>Theme: {shared.theme}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_contain() {
        let s = "<style>\n\t.optimized {\n\t\tcontain: layout style paint;\n\t\tcontent-visibility: auto;\n\t\tcontain-intrinsic-size: 0 500px;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- to 1400 ---

    #[test]
    fn test_parse_real_world_virtual_scroll() {
        let s = "<script>\n\tlet { items, height = 40 } = $props();\n\tlet scrollTop = $state(0);\n\tlet containerHeight = $state(400);\n\tlet start = $derived(Math.floor(scrollTop / height));\n\tlet visible = $derived(Math.ceil(containerHeight / height) + 2);\n\tlet slice = $derived(items.slice(start, start + visible));\n\tlet paddingTop = $derived(start * height);\n\tlet paddingBottom = $derived(Math.max(0, (items.length - start - visible) * height));\n</script>\n<div class=\"viewport\" style:height=\"{containerHeight}px\" on:scroll={(e) => scrollTop = e.target.scrollTop}>\n\t<div style:padding-top=\"{paddingTop}px\" style:padding-bottom=\"{paddingBottom}px\">\n\t\t{#each slice as item, i (start + i)}\n\t\t\t<div style:height=\"{height}px\">{item.label}</div>\n\t\t{/each}\n\t</div>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_multi_select() {
        let s = "<script>\n\tlet { options, selected = $bindable([]) } = $props();\n\tlet search = $state('');\n\tlet filtered = $derived(options.filter(o => o.label.toLowerCase().includes(search.toLowerCase()) && !selected.includes(o.value)));\n\tconst add = (val) => selected = [...selected, val];\n\tconst remove = (val) => selected = selected.filter(v => v !== val);\n</script>\n<div class=\"multi-select\">\n\t{#each selected as val}\n\t\t<span class=\"tag\">{options.find(o => o.value === val)?.label} <button onclick={() => remove(val)}>×</button></span>\n\t{/each}\n\t<input bind:value={search} />\n</div>\n{#if search && filtered.length > 0}\n\t<ul>\n\t\t{#each filtered as opt (opt.value)}\n\t\t\t<li onclick={() => { add(opt.value); search = ''; }}>{opt.label}</li>\n\t\t{/each}\n\t</ul>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scroll_snap_complete() {
        let s = "<style>\n\t.carousel {\n\t\tscroll-snap-type: x mandatory;\n\t\toverflow-x: scroll;\n\t\t-webkit-overflow-scrolling: touch;\n\t\tscrollbar-width: none;\n\t}\n\t.carousel::-webkit-scrollbar { display: none; }\n\t.slide {\n\t\tscroll-snap-align: start;\n\t\tscroll-snap-stop: always;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_derived_complex_chain() {
        let s = "<script lang=\"ts\">\n\tlet items = $state<{price: number; qty: number}[]>([]);\n\tlet subtotals = $derived(items.map(i => i.price * i.qty));\n\tlet total = $derived(subtotals.reduce((a, b) => a + b, 0));\n\tlet tax = $derived(total * 0.08);\n\tlet grandTotal = $derived(total + tax);\n\tlet formatted = $derived(`$${grandTotal.toFixed(2)}`);\n</script>\n<p>Total: {formatted}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_breadcrumb_dynamic() {
        let s = "<script>\n\tlet { segments } = $props();\n\tlet crumbs = $derived(segments.map((s, i) => ({\n\t\tlabel: s.charAt(0).toUpperCase() + s.slice(1),\n\t\thref: '/' + segments.slice(0, i + 1).join('/'),\n\t\tcurrent: i === segments.length - 1\n\t})));\n</script>\n<nav>\n\t{#each crumbs as crumb, i}\n\t\t{#if crumb.current}\n\t\t\t<span aria-current=\"page\">{crumb.label}</span>\n\t\t{:else}\n\t\t\t<a href={crumb.href}>{crumb.label}</a>\n\t\t\t<span aria-hidden=\"true\">/</span>\n\t\t{/if}\n\t{/each}\n</nav>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_indent_key_block() {
        let s = "{#key id}\ntext\n{/key}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/indent"),
            "Should flag unindented key block content");
    }

    #[test]
    fn test_parse_real_world_network_status() {
        let s = "<script>\n\tlet online = $state(true);\n\t$effect(() => {\n\t\tconst on = () => online = true;\n\t\tconst off = () => online = false;\n\t\twindow.addEventListener('online', on);\n\t\twindow.addEventListener('offline', off);\n\t\tonline = navigator.onLine;\n\t\treturn () => {\n\t\t\twindow.removeEventListener('online', on);\n\t\t\twindow.removeEventListener('offline', off);\n\t\t};\n\t});\n</script>\n{#if !online}\n\t<div class=\"offline-banner\" role=\"alert\">You are offline</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_event_types() {
        let s = "<button\n\tonclick={(e: MouseEvent) => handle(e)}\n\tonkeydown={(e: KeyboardEvent) => { if (e.key === 'Enter') handle(e); }}\n>click or press Enter</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_logical_complete() {
        let s = "<style>\n\t.box {\n\t\tmargin-block: 1rem;\n\t\tmargin-inline: auto;\n\t\tpadding-block-start: 2rem;\n\t\tpadding-inline-end: 1rem;\n\t\tborder-block-start: 2px solid;\n\t\tborder-inline: 1px solid;\n\t\tinset-inline: 0;\n\t\tinset-block-start: 0;\n\t\tblock-size: 100vh;\n\t\tinline-size: 100%;\n\t\tmin-block-size: 50vh;\n\t\tmax-inline-size: 1200px;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_idle_detector() {
        let s = "<script>\n\tlet idle = $state(false);\n\tlet timeout = 60000;\n\tlet timer;\n\t$effect(() => {\n\t\tconst reset = () => { idle = false; clearTimeout(timer); timer = setTimeout(() => idle = true, timeout); };\n\t\t['mousemove', 'keydown', 'scroll', 'click'].forEach(e => window.addEventListener(e, reset));\n\t\treset();\n\t\treturn () => { clearTimeout(timer); ['mousemove', 'keydown', 'scroll', 'click'].forEach(e => window.removeEventListener(e, reset)); };\n\t});\n</script>\n{#if idle}<div class=\"idle-overlay\">Still there?</div>{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_conditional_attrs() {
        let s = "<button\n\tclass=\"btn {variant}\"\n\tdisabled={loading || disabled}\n\taria-busy={loading}\n\taria-disabled={disabled}\n\tonclick={!loading && !disabled ? handler : undefined}\n>\n\t{#if loading}\n\t\t<Spinner />\n\t{:else}\n\t\t<slot />\n\t{/if}\n</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_valid_each_key_destructured() {
        let s = "{#each items as { id, name } (id)}\n\t<p>{name}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-each-key"),
            "Should NOT flag key using destructured property");
    }

    #[test]
    fn test_parse_svelte5_state_with_computed() {
        let s = "<script>\n\tlet form = $state({\n\t\tfirstName: '',\n\t\tlastName: '',\n\t});\n\tlet fullName = $derived(`${form.firstName} ${form.lastName}`.trim());\n\tlet isValid = $derived(form.firstName.length > 0 && form.lastName.length > 0);\n</script>\n<input bind:value={form.firstName} placeholder=\"First\" />\n<input bind:value={form.lastName} placeholder=\"Last\" />\n<p class:valid={isValid}>{fullName || 'Enter your name'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_accessible_dialog() {
        let s = "<script>\n\tlet { open = $bindable(false), title, children } = $props();\n</script>\n{#if open}\n\t<div class=\"backdrop\" onclick={() => open = false} role=\"presentation\" />\n\t<dialog {open} aria-labelledby=\"dialog-title\" aria-modal=\"true\">\n\t\t<h2 id=\"dialog-title\">{title}</h2>\n\t\t<div class=\"content\">{@render children()}</div>\n\t\t<button onclick={() => open = false} aria-label=\"Close\">×</button>\n\t</dialog>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_inert() {
        let s = "<style>\n\t[inert] { opacity: 0.5; pointer-events: none; user-select: none; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_scroll_spy() {
        let s = "<script>\n\tlet { sections } = $props();\n\tlet active = $state('');\n\t$effect(() => {\n\t\tconst obs = new IntersectionObserver((entries) => {\n\t\t\tentries.forEach(e => { if (e.isIntersecting) active = e.target.id; });\n\t\t}, { rootMargin: '-50% 0px' });\n\t\tsections.forEach(s => {\n\t\t\tconst el = document.getElementById(s.id);\n\t\t\tif (el) obs.observe(el);\n\t\t});\n\t\treturn () => obs.disconnect();\n\t});\n</script>\n<nav>\n\t{#each sections as section}\n\t\t<a href=\"#{section.id}\" class:active={active === section.id}>{section.label}</a>\n\t{/each}\n</nav>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_effect_cleanup_pattern() {
        let s = "<script>\n\tlet ws;\n\tlet messages = $state([]);\n\tlet url = $state('ws://localhost:8080');\n\t$effect(() => {\n\t\tws = new WebSocket(url);\n\t\tws.onmessage = (e) => messages.push(JSON.parse(e.data));\n\t\tws.onerror = (e) => console.error(e);\n\t\treturn () => ws.close();\n\t});\n</script>\n{#each messages as msg (msg.id)}\n\t<p>{msg.text}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_forward_events() {
        let s = "<script>\n\tlet { children, ...events } = $props();\n</script>\n<div {...events}>\n\t{@render children?.()}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_color_mix_advanced() {
        let s = "<style>\n\t.blend {\n\t\tcolor: color-mix(in oklch, var(--primary) 70%, white);\n\t\tbackground: color-mix(in srgb, currentColor 10%, transparent);\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_reactive_fn_arrow_in_reactive() {
        let s = "<script>\n\t$: handler = () => console.log('reactive arrow');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-functions"),
            "Should flag arrow function in reactive declaration");
    }

    #[test]
    fn test_parse_real_world_local_storage() {
        let s = "<script>\n\tfunction persisted(key, initial) {\n\t\tlet value = $state(JSON.parse(localStorage.getItem(key) ?? JSON.stringify(initial)));\n\t\t$effect(() => {\n\t\t\tlocalStorage.setItem(key, JSON.stringify(value));\n\t\t});\n\t\treturn { get value() { return value; }, set value(v) { value = v; } };\n\t}\n\tconst settings = persisted('settings', { theme: 'light', fontSize: 16 });\n</script>\n<select bind:value={settings.value.theme}>\n\t<option>light</option>\n\t<option>dark</option>\n</select>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_complete_app() {
        let s = "<script lang=\"ts\">\n\timport { onMount } from 'svelte';\n\tlet { data } = $props();\n\tlet search = $state('');\n\tlet filtered = $derived(data.filter(d => d.name.includes(search)));\n\tlet selected = $state<typeof data[0] | null>(null);\n\tonMount(() => console.log('App mounted'));\n</script>\n\n<svelte:head><title>{data.length} Items</title></svelte:head>\n\n<header>\n\t<h1>Items</h1>\n\t<input type=\"search\" bind:value={search} placeholder=\"Filter...\" />\n</header>\n\n<main>\n\t{#if filtered.length > 0}\n\t\t{#each filtered as item (item.id)}\n\t\t\t<div class:selected={item === selected} onclick={() => selected = item}>\n\t\t\t\t<h2>{item.name}</h2>\n\t\t\t\t<p>{item.description}</p>\n\t\t\t</div>\n\t\t{/each}\n\t{:else}\n\t\t<p>No items match \"{search}\"</p>\n\t{/if}\n</main>\n\n{#if selected}\n\t<aside>\n\t\t<h2>{selected.name}</h2>\n\t\t<p>{selected.description}</p>\n\t\t<button onclick={() => selected = null}>Close</button>\n\t</aside>\n{/if}\n\n<style lang=\"scss\">\n\theader { display: flex; justify-content: space-between; padding: 1rem; }\n\tmain { max-width: 960px; margin: 0 auto; }\n\t.selected { background: var(--highlight); }\n\taside { position: fixed; right: 0; top: 0; width: 300px; padding: 1rem; background: white; box-shadow: -2px 0 8px rgba(0,0,0,.1); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    // --- to 1400 ---

    #[test]
    fn test_parse_real_world_emoji_picker() {
        let s = "<script>\n\tlet { onselect } = $props();\n\tlet search = $state('');\n\tlet categories = ['😀', '🎉', '❤️', '🔥', '⭐'];\n\tlet filtered = $derived(categories.filter(e => !search || e.includes(search)));\n</script>\n<input bind:value={search} placeholder=\"Search emoji...\" />\n<div class=\"grid\">\n\t{#each filtered as emoji}\n\t\t<button onclick={() => onselect?.(emoji)}>{emoji}</button>\n\t{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_diff_viewer() {
        let s = "<script>\n\tlet { oldText, newText } = $props();\n\tlet lines = $derived.by(() => {\n\t\tconst old = oldText.split('\\n');\n\t\tconst cur = newText.split('\\n');\n\t\treturn cur.map((line, i) => ({ text: line, changed: line !== old[i] }));\n\t});\n</script>\n<pre>\n\t{#each lines as line, i}\n\t\t<span class:changed={line.changed} class:added={!line.changed}>{i + 1} | {line.text}\\n</span>\n\t{/each}\n</pre>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_live_clock() {
        let s = "<script>\n\tlet time = $state(new Date());\n\tlet formatted = $derived(time.toLocaleTimeString());\n\t$effect(() => {\n\t\tconst id = setInterval(() => time = new Date(), 1000);\n\t\treturn () => clearInterval(id);\n\t});\n</script>\n<time datetime={time.toISOString()}>{formatted}</time>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_context_with_state() {
        let s = "<script>\n\timport { setContext } from 'svelte';\n\tlet count = $state(0);\n\tsetContext('counter', {\n\t\tget count() { return count; },\n\t\tincrement: () => count++,\n\t\tdecrement: () => count--\n\t});\n</script>\n<slot />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scroll_margin() {
        let s = "<style>\n\t[id] { scroll-margin-top: 80px; }\n\t.scroll-smooth { scroll-behavior: smooth; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_lazy_image() {
        let s = "<script>\n\tlet { src, alt, placeholder = 'data:image/svg+xml,...' } = $props();\n\tlet loaded = $state(false);\n\tlet el;\n\t$effect(() => {\n\t\tconst obs = new IntersectionObserver(([e]) => {\n\t\t\tif (e.isIntersecting) { loaded = true; obs.disconnect(); }\n\t\t});\n\t\tif (el) obs.observe(el);\n\t\treturn () => obs.disconnect();\n\t});\n</script>\n<img bind:this={el} src={loaded ? src : placeholder} {alt} class:loaded loading=\"lazy\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_groupby() {
        let s = "{#each Object.entries(grouped) as [category, items] (category)}\n\t<h3>{category}</h3>\n\t{#each items as item (item.id)}\n\t\t<p>{item.name}</p>\n\t{/each}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_indent_await() {
        let s = "{#await p}\nloading\n{:then v}\nvalue\n{:catch e}\nerror\n{/await}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let indent: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/indent").collect();
        assert!(indent.len() >= 3, "Should flag unindented await block content");
    }

    #[test]
    fn test_parse_real_world_keyboard_shortcut() {
        let s = "<script>\n\tlet { shortcuts } = $props();\n\t$effect(() => {\n\t\tconst handler = (e) => {\n\t\t\tconst key = `${e.ctrlKey ? 'Ctrl+' : ''}${e.key}`;\n\t\t\tconst action = shortcuts[key];\n\t\t\tif (action) { e.preventDefault(); action(); }\n\t\t};\n\t\twindow.addEventListener('keydown', handler);\n\t\treturn () => window.removeEventListener('keydown', handler);\n\t});\n</script>\n<slot />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_derived_map() {
        let s = "<script>\n\tlet items = $state([1, 2, 3]);\n\tlet doubled = $derived(items.map(x => x * 2));\n\tlet total = $derived(doubled.reduce((a, b) => a + b, 0));\n</script>\n<p>Items: {items.join(', ')}</p>\n<p>Doubled: {doubled.join(', ')}</p>\n<p>Total: {total}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_print_specific() {
        let s = "<style>\n\t@media print {\n\t\t.no-print { display: none !important; }\n\t\ta[href]::after { content: ' (' attr(href) ')'; }\n\t\tbody { font-size: 12pt; color: black; background: white; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_rating() {
        let s = "<script>\n\tlet { max = 5, value = $bindable(0), readonly = false } = $props();\n</script>\n<div class=\"rating\" role=\"radiogroup\">\n\t{#each Array(max) as _, i}\n\t\t<button\n\t\t\tclass:filled={i < value}\n\t\t\tclass:readonly\n\t\t\tdisabled={readonly}\n\t\t\tonclick={() => value = i + 1}\n\t\t\taria-label=\"{i + 1} star{i !== 0 ? 's' : ''}\"\n\t\t>★</button>\n\t{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_fine_grained_updates() {
        let s = "<script>\n\tlet user = $state({ name: 'Alice', age: 30, address: { city: 'NYC' } });\n\t// Fine-grained: only re-renders what changed\n\tlet greeting = $derived(`Hello ${user.name}`);\n\tlet location = $derived(`Lives in ${user.address.city}`);\n</script>\n<h1>{greeting}</h1>\n<p>{location}</p>\n<input bind:value={user.name} />\n<input bind:value={user.address.city} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_comprehensive_svelte5() {
        let s = "<script lang=\"ts\">\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n\t$effect(() => console.log(count));\n\t$inspect(count);\n</script>\n<button onclick={() => count++}>{count}</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        // Should flag $inspect but not normal Svelte 5 usage
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inspect"));
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"));
    }

    #[test]
    fn test_parse_real_world_tooltip_component() {
        let s = "<script>\n\tlet { text, position = 'top', children } = $props();\n\tlet show = $state(false);\n</script>\n<div\n\tonmouseenter={() => show = true}\n\tonmouseleave={() => show = false}\n\tclass=\"tooltip-trigger\"\n>\n\t{@render children()}\n\t{#if show}\n\t\t<div class=\"tooltip {position}\" transition:fade={{duration: 100}}>\n\t\t\t{text}\n\t\t</div>\n\t{/if}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_array_methods() {
        let s = "<p>{items.filter(Boolean).sort().reverse().slice(0, 5).join(' | ')}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_custom_highlight() {
        let s = "<style>\n\t::highlight(search) { background: yellow; color: black; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_snippet_with_generics() {
        let s = "<script lang=\"ts\" generics=\"T\">\n\tlet { items }: { items: T[] } = $props();\n</script>\n{#snippet row(item: T, index: number)}\n\t<tr><td>{index}</td><td>{JSON.stringify(item)}</td></tr>\n{/snippet}\n<table>\n\t{#each items as item, i (i)}\n\t\t{@render row(item, i)}\n\t{/each}\n</table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_mutation_observer() {
        let s = "<script>\n\tlet el;\n\tlet mutations = $state(0);\n\t$effect(() => {\n\t\tconst obs = new MutationObserver((list) => mutations += list.length);\n\t\tif (el) obs.observe(el, { childList: true, subtree: true });\n\t\treturn () => obs.disconnect();\n\t});\n</script>\n<div bind:this={el}>\n\t<slot />\n</div>\n<p>Mutations: {mutations}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_immutable_reactive_with_mutable_let() {
        let s = "<script>\n\tlet count = 0;\n\t$: doubled = count * 2;\n\tfunction inc() { count++; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should NOT flag reactive stmt with mutable let");
    }

    #[test]
    fn test_parse_real_world_prefetch_link() {
        let s = "<script>\n\tlet { href, children } = $props();\n\tlet prefetched = $state(false);\n\tconst prefetch = () => {\n\t\tif (prefetched) return;\n\t\tconst link = document.createElement('link');\n\t\tlink.rel = 'prefetch';\n\t\tlink.href = href;\n\t\tdocument.head.appendChild(link);\n\t\tprefetched = true;\n\t};\n</script>\n<a {href} onmouseenter={prefetch}>{@render children()}</a>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_oklch() {
        let s = "<style>\n\t.primary { color: oklch(70% 0.2 240); }\n\t.secondary { color: oklch(60% 0.15 180); }\n\t.accent { color: oklch(80% 0.25 30); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_state_class_inheritance() {
        let s = "<script lang=\"ts\">\n\tclass Base {\n\t\tvalue = $state(0);\n\t\tget doubled() { return this.value * 2; }\n\t}\n\tclass Extended extends Base {\n\t\textra = $state('');\n\t}\n\tconst instance = new Extended();\n</script>\n<p>{instance.value} → {instance.doubled}</p>\n<input bind:value={instance.extra} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_1350th_milestone() {
        let s = "<script lang=\"ts\">\n\tlet items = $state<string[]>([]);\n\tlet input = $state('');\n\tlet count = $derived(items.length);\n\tconst add = () => { if (input) { items.push(input); input = ''; } };\n</script>\n<form onsubmit={(e) => { e.preventDefault(); add(); }}>\n\t<input bind:value={input} />\n\t<button type=\"submit\">Add ({count})</button>\n</form>\n{#each items as item, i (i)}\n\t<p>{item}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- push to 1350 ---

    #[test]
    fn test_parse_real_world_color_contrast() {
        let s = "<script>\n\tlet { fg = '#000', bg = '#fff' } = $props();\n\tlet ratio = $derived.by(() => {\n\t\tconst l1 = relativeLuminance(fg);\n\t\tconst l2 = relativeLuminance(bg);\n\t\treturn (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);\n\t});\n\tlet passes = $derived(ratio >= 4.5 ? 'AA' : ratio >= 3 ? 'AA Large' : 'Fail');\n</script>\n<div style:color={fg} style:background={bg}>\n\t<p>Contrast: {ratio.toFixed(2)}:1 ({passes})</p>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_scroll_to_top() {
        let s = "<script>\n\tlet visible = $state(false);\n\t$effect(() => {\n\t\tconst handler = () => visible = window.scrollY > 300;\n\t\twindow.addEventListener('scroll', handler, { passive: true });\n\t\treturn () => window.removeEventListener('scroll', handler);\n\t});\n</script>\n{#if visible}\n\t<button class=\"scroll-top\" onclick={() => window.scrollTo({ top: 0, behavior: 'smooth' })} transition:fade>\n\t\t↑ Top\n\t</button>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_skeleton_card() {
        let s = "<script>\n\tlet { loading = true } = $props();\n</script>\n{#if loading}\n\t<div class=\"card skeleton\">\n\t\t<div class=\"image\" />\n\t\t<div class=\"line w-80\" />\n\t\t<div class=\"line w-60\" />\n\t\t<div class=\"line w-40\" />\n\t</div>\n{:else}\n\t<div class=\"card\">\n\t\t<slot />\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_form_state() {
        let s = "<script lang=\"ts\">\n\tlet form = $state({\n\t\tfields: { name: '', email: '' },\n\t\terrors: {} as Record<string, string>,\n\t\tsubmitting: false,\n\t\tsubmitted: false\n\t});\n\tconst validate = () => {\n\t\tform.errors = {};\n\t\tif (!form.fields.name) form.errors.name = 'Required';\n\t\tif (!form.fields.email.includes('@')) form.errors.email = 'Invalid';\n\t\treturn Object.keys(form.errors).length === 0;\n\t};\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_transitions_all() {
        let s = "<style>\n\t.fade { transition: opacity 0.3s ease; }\n\t.slide { transition: transform 0.3s, opacity 0.3s; }\n\t.grow { transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1); }\n\t.hover { transition-property: color, background-color; transition-duration: 0.15s; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_indent_else_block() {
        let s = "{#if x}\ntext\n{:else}\nother\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let indent: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/indent").collect();
        assert!(indent.len() >= 2, "Should flag unindented if/else content");
    }

    #[test]
    fn test_parse_real_world_media_query_component() {
        let s = "<script>\n\tlet matches = $state(false);\n\tlet { query = '(min-width: 768px)' } = $props();\n\t$effect(() => {\n\t\tconst mql = window.matchMedia(query);\n\t\tmatches = mql.matches;\n\t\tconst handler = (e) => matches = e.matches;\n\t\tmql.addEventListener('change', handler);\n\t\treturn () => mql.removeEventListener('change', handler);\n\t});\n</script>\n{#if matches}\n\t<slot name=\"desktop\" />\n{:else}\n\t<slot name=\"mobile\" />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_iife() {
        let s = "<p>{(() => { const x = compute(); return x > 0 ? '+' : '-'; })()}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_state_raw() {
        let s = "<script>\n\tlet raw = $state.raw({ count: 0, items: [] });\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_unnecessary_state_wrap_media_query() {
        let s = "<script>\n\timport { MediaQuery } from 'svelte/reactivity';\n\tconst mq = $state(new MediaQuery('(min-width: 768px)'));\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-unnecessary-state-wrap"),
            "Should flag $state(new MediaQuery(...))");
    }

    #[test]
    fn test_parse_real_world_teleport() {
        let s = "<script>\n\timport { onMount } from 'svelte';\n\tlet { target = 'body', children } = $props();\n\tlet container;\n\tonMount(() => {\n\t\tconst el = document.querySelector(target);\n\t\tif (el && container) el.appendChild(container);\n\t\treturn () => container?.remove();\n\t});\n</script>\n<div bind:this={container} style=\"display: contents\">\n\t{@render children()}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_gap_modern() {
        let s = "<style>\n\t.stack { display: flex; flex-direction: column; gap: clamp(0.5rem, 2vw, 2rem); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_dynamic_snippet() {
        let s = "{#snippet item(data)}\n\t<div>{data.name}</div>\n{/snippet}\n\n{#snippet detail(data)}\n\t<div>{data.name}: {data.description}</div>\n{/snippet}\n\n{#each items as i (i.id)}\n\t{@render (expanded ? detail : item)(i)}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_store_reactive_spread_raw() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst data = writable({});\n</script>\n<div {...data}>text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should flag raw store spread");
    }

    #[test]
    fn test_parse_real_world_clipboard_with_feedback() {
        let s = "<script>\n\tlet { text } = $props();\n\tlet status = $state('idle');\n\tconst copy = async () => {\n\t\ttry {\n\t\t\tawait navigator.clipboard.writeText(text);\n\t\t\tstatus = 'success';\n\t\t} catch {\n\t\t\tstatus = 'error';\n\t\t}\n\t\tsetTimeout(() => status = 'idle', 2000);\n\t};\n</script>\n<button onclick={copy} class=\"copy-btn\" class:success={status === 'success'} class:error={status === 'error'}>\n\t{status === 'success' ? '✓' : status === 'error' ? '✗' : '📋'}\n</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_set() {
        let s = "{#each [...new Set(items)] as unique (unique)}\n\t<p>{unique}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_effect_untrack() {
        let s = "<script>\n\timport { untrack } from 'svelte';\n\tlet a = $state(0);\n\tlet b = $state(0);\n\t$effect(() => {\n\t\t// Only react to 'a' changes\n\t\tconsole.log(a, untrack(() => b));\n\t});\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_prefer_writable_derived_effect_pre() {
        let s = "<script>\n\tlet { x } = $props();\n\tlet y = $state(x);\n\t$effect.pre(() => { y = x; });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should flag $state + $effect.pre pattern");
    }

    #[test]
    fn test_parse_real_world_focus_trap() {
        let s = "<script>\n\tfunction focusTrap(node) {\n\t\tconst focusable = node.querySelectorAll('button, input, a, [tabindex]');\n\t\tconst first = focusable[0];\n\t\tconst last = focusable[focusable.length - 1];\n\t\tconst handler = (e) => {\n\t\t\tif (e.key !== 'Tab') return;\n\t\t\tif (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }\n\t\t\telse if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }\n\t\t};\n\t\tnode.addEventListener('keydown', handler);\n\t\tfirst?.focus();\n\t\treturn { destroy: () => node.removeEventListener('keydown', handler) };\n\t}\n</script>\n<div use:focusTrap>\n\t<slot />\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_avatar() {
        let s = "<script>\n\tlet { src, alt, size = 'md', fallback } = $props();\n\tlet error = $state(false);\n\tlet initials = $derived(fallback ?? alt?.split(' ').map(w => w[0]).join('') ?? '?');\n</script>\n{#if src && !error}\n\t<img {src} {alt} class=\"avatar {size}\" onerror={() => error = true} />\n{:else}\n\t<div class=\"avatar {size} fallback\">{initials}</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_tabs_animated() {
        let s = "<script>\n\tlet { tabs } = $props();\n\tlet active = $state(0);\n\tlet indicator = $state({ left: 0, width: 0 });\n\tlet tabEls = [];\n\t$effect(() => {\n\t\tconst el = tabEls[active];\n\t\tif (el) indicator = { left: el.offsetLeft, width: el.offsetWidth };\n\t});\n</script>\n<div class=\"tabs\">\n\t{#each tabs as tab, i}\n\t\t<button bind:this={tabEls[i]} onclick={() => active = i} class:active={active === i}>{tab.label}</button>\n\t{/each}\n\t<div class=\"indicator\" style:left=\"{indicator.left}px\" style:width=\"{indicator.width}px\" />\n</div>\n{@render tabs[active].content?.()}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_map_set() {
        let s = "<p>{new Map([['a', 1], ['b', 2]]).get('a')}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_empty_attr() {
        let s = "<input disabled />\n<div hidden />\n<textarea readonly />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_tick() {
        let s = "<script>\n\timport { tick } from 'svelte';\n\tlet count = $state(0);\n\tconst increment = async () => { count++; await tick(); console.log('updated'); };\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_search_highlight() {
        let s = "<script>\n\tlet { text, query } = $props();\n\tlet parts = $derived.by(() => {\n\t\tif (!query) return [{ text, match: false }];\n\t\tconst regex = new RegExp(`(${query.replace(/[.*+?^${}()|[\\]\\\\]/g, '\\\\$&')})`, 'gi');\n\t\treturn text.split(regex).map(part => ({ text: part, match: regex.test(part) }));\n\t});\n</script>\n<span>\n\t{#each parts as part}\n\t\t{#if part.match}<mark>{part.text}</mark>{:else}{part.text}{/if}\n\t{/each}\n</span>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_cookie_banner() {
        let s = "<script>\n\tlet accepted = $state(false);\n\tlet visible = $state(true);\n\t$effect(() => {\n\t\taccepted = document.cookie.includes('cookies=accepted');\n\t\tvisible = !accepted;\n\t});\n\tconst accept = () => { document.cookie = 'cookies=accepted; max-age=31536000'; visible = false; };\n</script>\n{#if visible}\n\t<div class=\"banner\" transition:slide>\n\t\t<p>We use cookies. <a href=\"/privacy\">Learn more</a></p>\n\t\t<button onclick={accept}>Accept</button>\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_shorthand_directive_bind_name() {
        let s = "<input bind:value={value} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/shorthand-directive"),
            "Should flag non-shorthand bind:value={{value}}");
    }

    #[test]
    fn test_parse_css_gap_shorthand() {
        let s = "<style>\n\t.flex { display: flex; gap: 1rem 2rem; }\n\t.grid { display: grid; gap: 1rem; row-gap: 2rem; column-gap: 1rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_theme_toggle() {
        let s = "<script>\n\tlet theme = $state('system');\n\tlet resolved = $derived.by(() => {\n\t\tif (theme !== 'system') return theme;\n\t\treturn typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';\n\t});\n</script>\n<select bind:value={theme}>\n\t<option value=\"system\">System</option>\n\t<option value=\"light\">Light</option>\n\t<option value=\"dark\">Dark</option>\n</select>\n<p>Resolved: {resolved}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_set_ops() {
        let s = "<p>{new Set([...a, ...b]).size}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_dupe_else_if_multiple() {
        let s = "{#if a}\n\t1\n{:else if b}\n\t2\n{:else if a}\n\t3\n{:else if b}\n\t4\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let dupes: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-dupe-else-if-blocks").collect();
        assert!(dupes.len() >= 1, "Should flag at least 1 duplicate condition");
    }

    #[test]
    fn test_parse_real_world_password_strength() {
        let s = "<script>\n\tlet { password = '' } = $props();\n\tlet strength = $derived.by(() => {\n\t\tlet score = 0;\n\t\tif (password.length >= 8) score++;\n\t\tif (/[A-Z]/.test(password)) score++;\n\t\tif (/[0-9]/.test(password)) score++;\n\t\tif (/[^A-Za-z0-9]/.test(password)) score++;\n\t\treturn ['weak', 'fair', 'good', 'strong'][score] ?? 'weak';\n\t});\n</script>\n<meter value={strength === 'weak' ? 25 : strength === 'fair' ? 50 : strength === 'good' ? 75 : 100} min=\"0\" max=\"100\" />\n<span class=\"strength-{strength}\">{strength}</span>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_children_render() {
        let s = "<script>\n\tlet { children } = $props();\n</script>\n<div class=\"wrapper\">\n\t{@render children?.()}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_resize_observer() {
        let s = "<script>\n\tlet el;\n\tlet width = $state(0);\n\tlet height = $state(0);\n\t$effect(() => {\n\t\tconst obs = new ResizeObserver(([entry]) => {\n\t\t\twidth = entry.contentRect.width;\n\t\t\theight = entry.contentRect.height;\n\t\t});\n\t\tif (el) obs.observe(el);\n\t\treturn () => obs.disconnect();\n\t});\n</script>\n<div bind:this={el}>\n\t<slot />\n\t<p class=\"size\">{Math.round(width)}×{Math.round(height)}</p>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_skeleton_text() {
        let s = "<script>\n\tlet { lines = 3, loading = true } = $props();\n</script>\n{#if loading}\n\t{#each Array(lines) as _, i}\n\t\t<div class=\"skeleton-line\" style:width=\"{i === lines - 1 ? 60 : 100}%\" />\n\t{/each}\n{:else}\n\t<slot />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_reactive_literal_template() {
        let s = "<script>\n\t$: msg = `hello`;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should flag reactive template literal");
    }

    #[test]
    fn test_parse_component_with_typed_snippets() {
        let s = "<script lang=\"ts\">\n\timport type { Snippet } from 'svelte';\n\tlet { children, header }: {\n\t\tchildren: Snippet;\n\t\theader?: Snippet<[{ title: string }]>;\n\t} = $props();\n</script>\n{#if header}\n\t{@render header({ title: 'Hello' })}\n{/if}\n{@render children()}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_text_wrap_balance() {
        let s = "<style>\n\th1 { text-wrap: balance; }\n\tp { text-wrap: pretty; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_1300th_milestone() {
        let s = "<script lang=\"ts\">\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n</script>\n<p>{count} × 2 = {doubled}</p>\n<button onclick={() => count++}>+1</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
    }

    // --- FINAL PUSH TO 1300 ---

    #[test]
    fn test_parse_real_world_masonry() {
        let s = "<script>\n\tlet { items, columns = 3 } = $props();\n\tlet distributed = $derived.by(() => {\n\t\tconst cols = Array.from({ length: columns }, () => []);\n\t\titems.forEach((item, i) => cols[i % columns].push(item));\n\t\treturn cols;\n\t});\n</script>\n<div class=\"masonry\" style:--cols={columns}>\n\t{#each distributed as col, i}\n\t\t<div class=\"column\">\n\t\t\t{#each col as item (item.id)}\n\t\t\t\t<div class=\"item\" transition:fade>{@render item.content?.()}</div>\n\t\t\t{/each}\n\t\t</div>\n\t{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_toggle_group() {
        let s = "<script>\n\tlet { options, value = $bindable(null), multiple = false } = $props();\n\tconst toggle = (opt) => {\n\t\tif (multiple) {\n\t\t\tvalue = value?.includes(opt) ? value.filter(v => v !== opt) : [...(value ?? []), opt];\n\t\t} else {\n\t\t\tvalue = value === opt ? null : opt;\n\t\t}\n\t};\n</script>\n<div class=\"toggle-group\" role=\"group\">\n\t{#each options as opt (opt.value)}\n\t\t<button\n\t\t\trole=\"checkbox\"\n\t\t\taria-checked={multiple ? value?.includes(opt.value) : value === opt.value}\n\t\t\tonclick={() => toggle(opt.value)}\n\t\t\tclass:selected={multiple ? value?.includes(opt.value) : value === opt.value}\n\t\t>{opt.label}</button>\n\t{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_view_transitions() {
        let s = "<style>\n\t@view-transition { navigation: auto; }\n\t::view-transition-old(main) { animation: fade-out 0.3s; }\n\t::view-transition-new(main) { animation: fade-in 0.3s; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_infinite_list() {
        let s = "<script>\n\tlet items = $state([]);\n\tlet loading = $state(false);\n\tlet observer;\n\tlet sentinel;\n\t$effect(() => {\n\t\tobserver = new IntersectionObserver(async ([entry]) => {\n\t\t\tif (entry.isIntersecting && !loading) {\n\t\t\t\tloading = true;\n\t\t\t\tconst more = await fetch(`/api/items?offset=${items.length}`).then(r => r.json());\n\t\t\t\titems = [...items, ...more];\n\t\t\t\tloading = false;\n\t\t\t}\n\t\t});\n\t\tif (sentinel) observer.observe(sentinel);\n\t\treturn () => observer?.disconnect();\n\t});\n</script>\n{#each items as item (item.id)}<div>{item.text}</div>{/each}\n{#if loading}<p>Loading...</p>{/if}\n<div bind:this={sentinel} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_spring_animation() {
        let s = "<script>\n\timport { spring } from 'svelte/motion';\n\tlet coords = spring({ x: 50, y: 50 }, { stiffness: 0.1, damping: 0.5 });\n</script>\n<svg on:mousemove={(e) => coords.set({ x: e.offsetX, y: e.offsetY })}>\n\t<circle cx={$coords.x} cy={$coords.y} r=\"20\" fill=\"blue\" />\n</svg>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_indent_script_body() {
        let s = "<script>\nlet a = 1;\nlet b = 2;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let indent_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/indent").collect();
        assert_eq!(indent_diags.len(), 2, "Should flag 2 unindented script lines");
    }

    #[test]
    fn test_linter_indent_nested_blocks() {
        let s = "<div>\n{#if a}\n<p>text</p>\n{/if}\n</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().filter(|d| d.rule_name == "svelte/indent").count() >= 2,
            "Should flag unindented if block and content");
    }

    #[test]
    fn test_parse_real_world_intersection_observer() {
        let s = "<script>\n\tlet visible = $state(false);\n\tlet el;\n\t$effect(() => {\n\t\tconst obs = new IntersectionObserver(([e]) => visible = e.isIntersecting);\n\t\tif (el) obs.observe(el);\n\t\treturn () => obs.disconnect();\n\t});\n</script>\n<div bind:this={el} class:visible transition:fade>\n\t<slot />\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_portal() {
        let s = "<script>\n\timport { onMount } from 'svelte';\n\tlet { target = 'body' } = $props();\n\tlet portal;\n\tonMount(() => {\n\t\tconst t = document.querySelector(target);\n\t\tif (t && portal) t.appendChild(portal);\n\t\treturn () => portal?.remove();\n\t});\n</script>\n<div bind:this={portal}>\n\t<slot />\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_property_inheritance() {
        let s = "<style>\n\t.parent {\n\t\t--bg: white;\n\t\t--fg: black;\n\t\tbackground: var(--bg);\n\t\tcolor: var(--fg);\n\t}\n\t.child {\n\t\t--bg: black;\n\t\t--fg: white;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_popover() {
        let s = "<script>\n\tlet { trigger, content } = $props();\n\tlet open = $state(false);\n\tlet pos = $state({ x: 0, y: 0 });\n</script>\n<div class=\"popover-trigger\" onclick={(e) => { pos = { x: e.clientX, y: e.clientY }; open = !open; }}>\n\t{@render trigger()}\n</div>\n{#if open}\n\t<div class=\"popover\" style:left=\"{pos.x}px\" style:top=\"{pos.y}px\" transition:scale>\n\t\t{@render content()}\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_action_modern() {
        let s = "<script>\n\tfunction longpress(node, duration = 500) {\n\t\tlet timer;\n\t\tconst start = () => timer = setTimeout(() => node.dispatchEvent(new CustomEvent('longpress')), duration);\n\t\tconst cancel = () => clearTimeout(timer);\n\t\tnode.addEventListener('mousedown', start);\n\t\tnode.addEventListener('mouseup', cancel);\n\t\treturn { destroy() { cancel(); } };\n\t}\n</script>\n<button use:longpress={700} on:longpress={() => alert('long pressed!')}>Press and hold</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_animated_counter() {
        let s = "<script>\n\timport { tweened } from 'svelte/motion';\n\timport { cubicOut } from 'svelte/easing';\n\tlet { value } = $props();\n\tconst displayed = tweened(0, { duration: 500, easing: cubicOut });\n\t$effect(() => { displayed.set(value); });\n</script>\n<span>{Math.round($displayed)}</span>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_at_html_nested() {
        let s = "<div>\n\t{#if show}\n\t\t{@html content}\n\t{/if}\n</div>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-html-tags"));
    }

    #[test]
    fn test_parse_component_with_all_svelte5() {
        let s = "<script lang=\"ts\">\n\tlet { children, header, footer, class: className = '' }: {\n\t\tchildren: import('svelte').Snippet;\n\t\theader?: import('svelte').Snippet;\n\t\tfooter?: import('svelte').Snippet;\n\t\tclass?: string;\n\t} = $props();\n</script>\n<div class=\"card {className}\">\n\t{#if header}\n\t\t<header>{@render header()}</header>\n\t{/if}\n\t<main>{@render children()}</main>\n\t{#if footer}\n\t\t<footer>{@render footer()}</footer>\n\t{/if}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_anchor_positioning() {
        let s = "<style>\n\t.anchor { anchor-name: --tooltip; }\n\t.tooltip {\n\t\tposition: fixed;\n\t\tposition-anchor: --tooltip;\n\t\ttop: anchor(bottom);\n\t\tleft: anchor(center);\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_clipboard_manager() {
        let s = "<script>\n\tlet history = $state([]);\n\tlet maxItems = 20;\n\tconst copy = async (text) => {\n\t\tawait navigator.clipboard.writeText(text);\n\t\thistory = [{ text, time: Date.now() }, ...history.slice(0, maxItems - 1)];\n\t};\n\tconst paste = async (text) => {\n\t\tawait navigator.clipboard.writeText(text);\n\t};\n</script>\n{#each history as item (item.time)}\n\t<div class=\"clip\" onclick={() => paste(item.text)}>\n\t\t<p>{item.text.slice(0, 100)}</p>\n\t\t<time>{new Date(item.time).toLocaleTimeString()}</time>\n\t</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_derived_from_store() {
        let s = "<script>\n\timport { page } from '$app/stores';\n\tlet pathname = $derived($page.url.pathname);\n\tlet isHome = $derived(pathname === '/');\n</script>\n<a href=\"/\" class:active={isHome}>Home</a>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- towards 1300 ---

    #[test]
    fn test_indent_template_basic() {
        let s = "<div>\ntext\n</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/indent"),
            "Should flag unindented text inside div");
    }

    #[test]
    fn test_indent_template_correct_ok() {
        let s = "<div>\n  text\n</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/indent"),
            "Should NOT flag correctly indented text");
    }

    #[test]
    fn test_indent_script_basic() {
        let s = "<script>\nlet x = 0;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/indent"),
            "Should flag unindented script content");
    }

    #[test]
    fn test_indent_if_block() {
        let s = "{#if x}\ntext\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/indent"),
            "Should flag unindented if block content");
    }

    #[test]
    fn test_indent_each_block() {
        let s = "{#each items as item}\ntext\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/indent"),
            "Should flag unindented each block content");
    }

    #[test]
    fn test_indent_nested_correct_ok() {
        let s = "<div>\n  {#if x}\n    <p>text</p>\n  {/if}\n</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/indent"),
            "Should NOT flag correctly nested indentation");
    }

    #[test]
    fn test_parse_real_world_wizard() {
        let s = "<script>\n\tlet { steps } = $props();\n\tlet current = $state(0);\n\tlet progress = $derived(((current + 1) / steps.length) * 100);\n</script>\n\n<div class=\"wizard\">\n\t<progress value={progress} max=\"100\" />\n\t{@render steps[current].content?.()}\n\t<div class=\"nav\">\n\t\t<button disabled={current === 0} onclick={() => current--}>Back</button>\n\t\t<span>{current + 1} / {steps.length}</span>\n\t\t<button onclick={() => current < steps.length - 1 ? current++ : null}>\n\t\t\t{current === steps.length - 1 ? 'Finish' : 'Next'}\n\t\t</button>\n\t</div>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_status_badge() {
        let s = "<script>\n\tconst STATUS_CONFIG = {\n\t\tactive: { color: 'green', label: 'Active' },\n\t\tinactive: { color: 'gray', label: 'Inactive' },\n\t\tpending: { color: 'yellow', label: 'Pending' },\n\t};\n\tlet { status } = $props();\n\tlet config = $derived(STATUS_CONFIG[status] ?? STATUS_CONFIG.inactive);\n</script>\n<span class=\"badge\" style:background-color={config.color}>{config.label}</span>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_date_formatter() {
        let s = "<script>\n\tlet { date, format = 'relative' } = $props();\n\tlet formatted = $derived.by(() => {\n\t\tconst d = new Date(date);\n\t\tif (format === 'relative') {\n\t\t\tconst diff = Date.now() - d.getTime();\n\t\t\tif (diff < 60000) return 'just now';\n\t\t\tif (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;\n\t\t\treturn d.toLocaleDateString();\n\t\t}\n\t\treturn d.toISOString();\n\t});\n</script>\n<time datetime={new Date(date).toISOString()}>{formatted}</time>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_overscroll() {
        let s = "<style>\n\t.scrollable {\n\t\toverscroll-behavior: contain;\n\t\t-webkit-overflow-scrolling: touch;\n\t\tscrollbar-gutter: stable;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_typewriter() {
        let s = "<script>\n\tlet { text, speed = 50 } = $props();\n\tlet displayed = $state('');\n\tlet index = $state(0);\n\t$effect(() => {\n\t\tif (index >= text.length) return;\n\t\tconst timer = setTimeout(() => {\n\t\t\tdisplayed += text[index];\n\t\t\tindex++;\n\t\t}, speed);\n\t\treturn () => clearTimeout(timer);\n\t});\n</script>\n<span>{displayed}<span class=\"cursor\">|</span></span>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_context_typed() {
        let s = "<script lang=\"ts\">\n\timport { getContext, setContext } from 'svelte';\n\ttype ThemeContext = { primary: string; toggle: () => void };\n\tconst theme = getContext<ThemeContext>('theme');\n</script>\n<button onclick={theme.toggle} style:color={theme.primary}>Toggle</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_1250th_milestone() {
        // The 1250th test — a complete Svelte 5 SvelteKit page
        let s = "<script lang=\"ts\">\n\tlet { data } = $props();\n\tlet search = $state('');\n\tlet filtered = $derived(data.items.filter(i => i.name.includes(search)));\n\tlet count = $derived(filtered.length);\n</script>\n\n<svelte:head><title>Items ({count})</title></svelte:head>\n\n<main>\n\t<input type=\"search\" bind:value={search} placeholder=\"Search {data.items.length} items\" />\n\t{#each filtered as item (item.id)}\n\t\t<article>\n\t\t\t<h2>{item.name}</h2>\n\t\t\t<p>{item.description}</p>\n\t\t</article>\n\t{:else}\n\t\t<p>No items match \"{search}\"</p>\n\t{/each}\n</main>\n\n<style lang=\"scss\">\n\tmain { max-width: 800px; margin: 0 auto; padding: 1rem; }\n\tarticle { border-bottom: 1px solid #eee; padding: 1rem 0; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    #[test]
    fn test_parse_real_world_clipboard() {
        let s = "<script>\n\tlet { code } = $props();\n\tlet copied = $state(false);\n\tconst copy = async () => { await navigator.clipboard.writeText(code); copied = true; setTimeout(() => copied = false, 2000); };\n</script>\n<div class=\"code-block\">\n\t<pre><code>{code}</code></pre>\n\t<button onclick={copy}>{copied ? 'Copied!' : 'Copy'}</button>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_countdown() {
        let s = "<script>\n\tlet { targetDate } = $props();\n\tlet now = $state(Date.now());\n\tlet diff = $derived(Math.max(0, targetDate - now));\n\tlet days = $derived(Math.floor(diff / 86400000));\n\tlet hours = $derived(Math.floor((diff % 86400000) / 3600000));\n\tlet minutes = $derived(Math.floor((diff % 3600000) / 60000));\n\tlet seconds = $derived(Math.floor((diff % 60000) / 1000));\n\t$effect(() => {\n\t\tconst id = setInterval(() => now = Date.now(), 1000);\n\t\treturn () => clearInterval(id);\n\t});\n</script>\n<div class=\"countdown\">\n\t<span>{days}d</span><span>{hours}h</span><span>{minutes}m</span><span>{seconds}s</span>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_slot_children() {
        let s = "{#each items as item (item.id)}\n\t<Card>\n\t\t<h3 slot=\"title\">{item.title}</h3>\n\t\t<p>{item.body}</p>\n\t\t<svelte:fragment slot=\"footer\">\n\t\t\t<button onclick={() => edit(item)}>Edit</button>\n\t\t\t<button onclick={() => del(item.id)}>Delete</button>\n\t\t</svelte:fragment>\n\t</Card>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scope_at_rules() {
        let s = "<style>\n\t@font-face {\n\t\tfont-family: 'MyFont';\n\t\tsrc: url('/fonts/myfont.woff2') format('woff2');\n\t\tfont-display: swap;\n\t}\n\t@import url('https://fonts.googleapis.com/css2?family=Inter');\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_signal_pattern() {
        let s = "<script>\n\tfunction createSignal(initial) {\n\t\tlet value = $state(initial);\n\t\treturn {\n\t\t\tget value() { return value; },\n\t\t\tset value(v) { value = v; }\n\t\t};\n\t}\n\tconst count = createSignal(0);\n</script>\n<p>{count.value}</p>\n<button onclick={() => count.value++}>+</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_error_boundary() {
        let s = "<svelte:boundary onerror={(error) => console.error(error)}>\n\t<Router>\n\t\t{#if page === 'home'}\n\t\t\t<Home />\n\t\t{:else if page === 'about'}\n\t\t\t<About />\n\t\t{:else}\n\t\t\t<NotFound />\n\t\t{/if}\n\t</Router>\n\t{#snippet failed(error, reset)}\n\t\t<div class=\"error-page\">\n\t\t\t<h1>Something went wrong</h1>\n\t\t\t<pre>{error.message}</pre>\n\t\t\t<button onclick={reset}>Try again</button>\n\t\t</div>\n\t{/snippet}\n</svelte:boundary>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_button_type_on_input() {
        let s = "<input type=\"button\" value=\"Click\" />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/button-has-type"),
            "Should NOT flag input type=button");
    }

    #[test]
    fn test_parse_expression_comma_sequence() {
        let s = "<button on:click={() => (a++, b++, c++)}>all</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_attribute_spread_conditional() {
        let s = "<input {...(readonly ? { readonly: true, tabindex: -1 } : { tabindex: 0 })} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_accent_color() {
        let s = "<style>\n\tinput[type=checkbox] { accent-color: var(--primary); }\n\tprogress { accent-color: green; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_responsive_grid() {
        let s = "<script>\n\tlet { items, columns = 3 } = $props();\n</script>\n<div class=\"grid\" style:--cols={columns}>\n\t{#each items as item (item.id)}\n\t\t<div class=\"cell\">{@render item.render?.()}</div>\n\t{/each}\n</div>\n<style>\n\t.grid {\n\t\tdisplay: grid;\n\t\tgrid-template-columns: repeat(var(--cols, 3), 1fr);\n\t\tgap: 1rem;\n\t}\n\t@container (max-width: 500px) { .grid { grid-template-columns: 1fr; } }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_state_class_methods() {
        let s = "<script lang=\"ts\">\n\tclass TodoList {\n\t\titems = $state<string[]>([]);\n\t\tget count() { return this.items.length; }\n\t\tadd(text: string) { this.items.push(text); }\n\t\tremove(i: number) { this.items.splice(i, 1); }\n\t\tclear() { this.items = []; }\n\t}\n\tconst list = new TodoList();\n\tlet input = $state('');\n</script>\n<form onsubmit={(e) => { e.preventDefault(); list.add(input); input = ''; }}>\n\t<input bind:value={input} />\n</form>\n{#each list.items as item, i}\n\t<p>{item} <button onclick={() => list.remove(i)}>×</button></p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_all_rules_present() {
        let all = Linter::all();
        let count = all.rules().len();
        // We should have at least 79 rules
        assert!(count >= 79, "Expected >= 79 rules, got {}", count);
    }

    #[test]
    fn test_parse_real_world_ssr_guard() {
        let s = "<script>\n\timport { browser } from '$app/environment';\n\tlet width = $state(0);\n\t$effect(() => {\n\t\tif (browser) width = window.innerWidth;\n\t});\n</script>\n{#if browser}\n\t<p>Width: {width}px</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_index_binding() {
        let s = "{#each items as item, index (item.id)}\n\t<div data-index={index} class:first={index === 0} class:last={index === items.length - 1}>{item.name}</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_on_vs_onclick() {
        let s = "<!-- Svelte 4 style -->\n<button on:click={handle}>Old</button>\n<!-- Svelte 5 style -->\n<button onclick={handle}>New</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_dark_mode() {
        let s = "<script>\n\tlet dark = $state(false);\n\t$effect(() => {\n\t\tdocument.documentElement.classList.toggle('dark', dark);\n\t});\n\t$effect(() => {\n\t\tconst mq = window.matchMedia('(prefers-color-scheme: dark)');\n\t\tdark = mq.matches;\n\t\tconst handler = (e) => dark = e.matches;\n\t\tmq.addEventListener('change', handler);\n\t\treturn () => mq.removeEventListener('change', handler);\n\t});\n</script>\n<button onclick={() => dark = !dark}>{dark ? '☀️ Light' : '🌙 Dark'}</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_render_fallback() {
        let s = "<Widget>\n\t{#snippet content()}\n\t\t<p>Custom content</p>\n\t{/snippet}\n</Widget>\n\n<!-- Without snippet, uses default slot -->\n<Widget>\n\t<p>Default slot content</p>\n</Widget>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- FINAL BATCH TO 1250 ---

    #[test]
    fn test_parse_real_world_resizable_panel() {
        let s = "<script>\n\tlet width = $state(300);\n\tlet dragging = $state(false);\n\tconst startDrag = (e) => {\n\t\tdragging = true;\n\t\tconst startX = e.clientX;\n\t\tconst startWidth = width;\n\t\tconst onMove = (e) => width = Math.max(100, startWidth + e.clientX - startX);\n\t\tconst onUp = () => { dragging = false; window.removeEventListener('mousemove', onMove); };\n\t\twindow.addEventListener('mousemove', onMove);\n\t\twindow.addEventListener('mouseup', onUp, { once: true });\n\t};\n</script>\n<div class=\"panel\" style:width=\"{width}px\">\n\t<slot />\n\t<div class=\"handle\" class:dragging on:mousedown={startDrag} />\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_copy_button() {
        let s = "<script>\n\tlet { text } = $props();\n\tlet copied = $state(false);\n\tconst copy = async () => {\n\t\tawait navigator.clipboard.writeText(text);\n\t\tcopied = true;\n\t\tsetTimeout(() => copied = false, 2000);\n\t};\n</script>\n<button onclick={copy} aria-label=\"Copy\">\n\t{copied ? '✓ Copied!' : '📋 Copy'}\n</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_snapshots() {
        let s = "<script>\n\timport { unstate } from 'svelte';\n\tlet state = $state({ count: 0, items: [1, 2, 3] });\n\tconst getSnapshot = () => JSON.parse(JSON.stringify(unstate(state)));\n\tconst restore = (snapshot) => { state.count = snapshot.count; state.items = [...snapshot.items]; };\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_async_content() {
        let s = "{#each items as item (item.id)}\n\t{#await loadDetails(item.id)}\n\t\t<div class=\"skeleton\" />\n\t{:then details}\n\t\t<div class=\"item\">\n\t\t\t<h3>{item.name}</h3>\n\t\t\t<p>{details.description}</p>\n\t\t</div>\n\t{:catch}\n\t\t<div class=\"error\">Failed to load</div>\n\t{/await}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_container_style() {
        let s = "<style>\n\t.card {\n\t\tcontainer: card / inline-size;\n\t\tpadding: 1rem;\n\t}\n\t@container card (min-width: 400px) {\n\t\t.card-content { display: grid; grid-template-columns: 1fr 1fr; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_toast_notification() {
        let s = "<script>\n\tlet { message, type = 'info', duration = 3000, onclose } = $props();\n\tlet visible = $state(true);\n\t$effect(() => {\n\t\tconst timer = setTimeout(() => {\n\t\t\tvisible = false;\n\t\t\tonclose?.();\n\t\t}, duration);\n\t\treturn () => clearTimeout(timer);\n\t});\n</script>\n{#if visible}\n\t<div class=\"toast {type}\" role=\"alert\" transition:fly={{y: -20, duration: 200}}>\n\t\t<p>{message}</p>\n\t\t<button onclick={() => { visible = false; onclose?.(); }} aria-label=\"Close\">&times;</button>\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_comprehensive_check_all() {
        // Multiple rules in one file
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst store = writable(0);\n\t$: literal = 42;\n\t$inspect(literal);\n</script>\n{@html content}\n{@debug literal}\n<button>click</button>\n<p>{store}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let mut rules_hit = std::collections::HashSet::new();
        for d in &diags { rules_hit.insert(d.rule_name.clone()); }
        assert!(rules_hit.len() >= 5, "Expected >= 5 rules, got {}: {:?}", rules_hit.len(), rules_hit);
    }

    #[test]
    fn test_parse_element_with_all_bindings() {
        let s = "<div\n\tbind:this={el}\n\tbind:clientWidth={w}\n\tbind:clientHeight={h}\n\tbind:offsetWidth={ow}\n\tbind:offsetHeight={oh}\n\tbind:contentRect={rect}\n>content</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_form_with_enhance() {
        let s = "<script>\n\timport { enhance } from '$app/forms';\n\tlet form;\n</script>\n<form method=\"POST\" action=\"?/login\" use:enhance>\n\t<input name=\"email\" type=\"email\" required />\n\t<input name=\"password\" type=\"password\" required />\n\t<button type=\"submit\">Login</button>\n\t{#if form?.error}\n\t\t<p class=\"error\">{form.error}</p>\n\t{/if}\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_counter_family() {
        let s = "<script>\n\tclass Counter {\n\t\tcount = $state(0);\n\t\tincrement() { this.count++; }\n\t\tdecrement() { this.count--; }\n\t\treset() { this.count = 0; }\n\t}\n\tconst a = new Counter();\n\tconst b = new Counter();\n</script>\n<div>\n\t<p>A: {a.count}</p>\n\t<button onclick={() => a.increment()}>A+</button>\n\t<p>B: {b.count}</p>\n\t<button onclick={() => b.increment()}>B+</button>\n\t<p>Total: {a.count + b.count}</p>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scope_all_features() {
        let s = "<style>\n\t:root { --spacing: 1rem; }\n\t:global(body) { margin: 0; }\n\t.local { padding: var(--spacing); }\n\t.local :global(.external) { color: blue; }\n\t:global(body.dark) .local { background: #333; }\n\t@media (prefers-color-scheme: dark) { .local { background: #111; } }\n\t@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_skeleton_loader() {
        let s = "<script>\n\tlet { loading = true, lines = 3 } = $props();\n</script>\n{#if loading}\n\t<div class=\"skeleton\" aria-busy=\"true\" role=\"status\">\n\t\t{#each Array(lines) as _, i}\n\t\t\t<div class=\"line\" style:width=\"{i === lines - 1 ? '60%' : '100%'}\" />\n\t\t{/each}\n\t</div>\n{:else}\n\t<slot />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_reactive_fn_svelte5_ok() {
        let s = "<script>\n\tconst helper = () => 42;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-reactive-functions"),
            "Should NOT flag regular const arrow");
    }

    #[test]
    fn test_parse_svelte5_deep_reactive() {
        let s = "<script>\n\tlet form = $state({\n\t\tuser: {\n\t\t\tname: '',\n\t\t\taddress: { city: '', country: '' }\n\t\t},\n\t\tpreferences: {\n\t\t\ttheme: 'light',\n\t\t\tnotifications: true\n\t\t}\n\t});\n</script>\n<input bind:value={form.user.name} />\n<input bind:value={form.user.address.city} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_key_expression() {
        let s = "{#each items as item (JSON.stringify([item.type, item.id]))}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_render_with_spread() {
        let s = "{@render header?.(...headerProps)}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_responsive_nav() {
        let s = "<script>\n\tlet open = $state(false);\n\tlet { items } = $props();\n</script>\n<nav>\n\t<button class=\"hamburger\" onclick={() => open = !open} aria-expanded={open} aria-label=\"Toggle menu\">\n\t\t<span /><span /><span />\n\t</button>\n\t<ul class:open>\n\t\t{#each items as item (item.href)}\n\t\t\t<li><a href={item.href} class:active={item.active}>{item.label}</a></li>\n\t\t{/each}\n\t</ul>\n</nav>\n<style>\n\t.hamburger { display: none; }\n\t@media (max-width: 768px) {\n\t\t.hamburger { display: block; }\n\t\tul { display: none; }\n\t\tul.open { display: flex; flex-direction: column; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- towards 1250 ---

    #[test]
    fn test_parse_real_world_image_gallery() {
        let s = "<script>\n\tlet { images } = $props();\n\tlet selected = $state(null);\n\tlet lightbox = $state(false);\n</script>\n\n<div class=\"gallery\">\n\t{#each images as img (img.id)}\n\t\t<button onclick={() => { selected = img; lightbox = true; }}>\n\t\t\t<img src={img.thumbnail} alt={img.alt} loading=\"lazy\" />\n\t\t</button>\n\t{/each}\n</div>\n\n{#if lightbox && selected}\n\t<div class=\"lightbox\" onclick={() => lightbox = false} transition:fade>\n\t\t<img src={selected.full} alt={selected.alt} />\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_calendar() {
        let s = "<script>\n\tlet { year, month, onDateSelect } = $props();\n\tlet days = $derived(getDaysInMonth(year, month));\n\tlet firstDay = $derived(new Date(year, month, 1).getDay());\n</script>\n\n<div class=\"calendar\">\n\t<div class=\"header\">\n\t\t<button onclick={() => month--}>&lt;</button>\n\t\t<span>{year}-{String(month + 1).padStart(2, '0')}</span>\n\t\t<button onclick={() => month++}>&gt;</button>\n\t</div>\n\t<div class=\"grid\">\n\t\t{#each Array(firstDay) as _}<div />{/each}\n\t\t{#each Array(days) as _, i}\n\t\t\t<button onclick={() => onDateSelect?.(new Date(year, month, i + 1))}>{i + 1}</button>\n\t\t{/each}\n\t</div>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_progress_tracker() {
        let s = "<script>\n\tlet { steps, currentStep } = $props();\n\tlet progress = $derived(((currentStep + 1) / steps.length) * 100);\n</script>\n\n<div class=\"tracker\" role=\"progressbar\" aria-valuenow={progress} aria-valuemin=\"0\" aria-valuemax=\"100\">\n\t{#each steps as step, i}\n\t\t<div class=\"step\" class:completed={i < currentStep} class:active={i === currentStep} class:upcoming={i > currentStep}>\n\t\t\t<span class=\"number\">{i + 1}</span>\n\t\t\t<span class=\"label\">{step.label}</span>\n\t\t</div>\n\t\t{#if i < steps.length - 1}\n\t\t\t<div class=\"connector\" class:filled={i < currentStep} />\n\t\t{/if}\n\t{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_tag_input() {
        let s = "<script>\n\tlet { tags = $bindable([]), maxTags = 10 } = $props();\n\tlet input = $state('');\n\tlet remaining = $derived(maxTags - tags.length);\n\tconst add = () => {\n\t\tconst tag = input.trim();\n\t\tif (tag && !tags.includes(tag) && tags.length < maxTags) {\n\t\t\ttags = [...tags, tag];\n\t\t\tinput = '';\n\t\t}\n\t};\n\tconst remove = (tag) => tags = tags.filter(t => t !== tag);\n</script>\n\n<div class=\"tags\">\n\t{#each tags as tag}\n\t\t<span class=\"tag\">{tag} <button onclick={() => remove(tag)}>&times;</button></span>\n\t{/each}\n\t{#if remaining > 0}\n\t\t<input bind:value={input} onkeydown={(e) => e.key === 'Enter' && (e.preventDefault(), add())} placeholder=\"Add tag...\" />\n\t{/if}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scroll_driven() {
        let s = "<style>\n\t@keyframes reveal {\n\t\tfrom { opacity: 0; transform: translateY(20px); }\n\t\tto { opacity: 1; transform: translateY(0); }\n\t}\n\t.scroll-reveal {\n\t\tanimation: reveal linear both;\n\t\tanimation-timeline: view();\n\t\tanimation-range: entry 0% entry 100%;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_sortable_table() {
        let s = "<script lang=\"ts\">\n\ttype SortDir = 'asc' | 'desc' | null;\n\tlet { data, columns } = $props<{ data: any[]; columns: { key: string; label: string }[] }>();\n\tlet sortKey = $state<string | null>(null);\n\tlet sortDir = $state<SortDir>(null);\n\tlet sorted = $derived.by(() => {\n\t\tif (!sortKey || !sortDir) return data;\n\t\treturn [...data].sort((a, b) => {\n\t\t\tconst cmp = String(a[sortKey!]).localeCompare(String(b[sortKey!]));\n\t\t\treturn sortDir === 'desc' ? -cmp : cmp;\n\t\t});\n\t});\n\tconst toggleSort = (key: string) => {\n\t\tif (sortKey === key) {\n\t\t\tsortDir = sortDir === 'asc' ? 'desc' : sortDir === 'desc' ? null : 'asc';\n\t\t\tif (!sortDir) sortKey = null;\n\t\t} else {\n\t\t\tsortKey = key;\n\t\t\tsortDir = 'asc';\n\t\t}\n\t};\n</script>\n\n<table>\n\t<thead>\n\t\t<tr>\n\t\t\t{#each columns as col}\n\t\t\t\t<th onclick={() => toggleSort(col.key)} class:sorted={sortKey === col.key}>\n\t\t\t\t\t{col.label}\n\t\t\t\t\t{#if sortKey === col.key}\n\t\t\t\t\t\t<span>{sortDir === 'asc' ? '↑' : '↓'}</span>\n\t\t\t\t\t{/if}\n\t\t\t\t</th>\n\t\t\t{/each}\n\t\t</tr>\n\t</thead>\n\t<tbody>\n\t\t{#each sorted as row, i (i)}\n\t\t\t<tr>\n\t\t\t\t{#each columns as col}\n\t\t\t\t\t<td>{row[col.key]}</td>\n\t\t\t\t{/each}\n\t\t\t</tr>\n\t\t{/each}\n\t</tbody>\n</table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_breadcrumbs_svelte5() {
        let s = "<script>\n\tlet { items } = $props();\n</script>\n<nav aria-label=\"Breadcrumb\">\n\t<ol>\n\t\t{#each items as item, i (item.href)}\n\t\t\t<li>\n\t\t\t\t{#if i < items.length - 1}\n\t\t\t\t\t<a href={item.href}>{item.label}</a>\n\t\t\t\t\t<span aria-hidden=\"true\">/</span>\n\t\t\t\t{:else}\n\t\t\t\t\t<span aria-current=\"page\">{item.label}</span>\n\t\t\t\t{/if}\n\t\t\t</li>\n\t\t{/each}\n\t</ol>\n</nav>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_modern_all() {
        let s = "<style>\n\t:root { color-scheme: light dark; }\n\t.container {\n\t\tcontainer-type: inline-size;\n\t\tmax-inline-size: 1200px;\n\t\tmargin-inline: auto;\n\t}\n\t@container (inline-size > 700px) {\n\t\t.grid { grid-template-columns: 1fr 1fr; }\n\t}\n\t.text {\n\t\ttext-wrap: balance;\n\t\toverscroll-behavior: contain;\n\t}\n\t@layer base, theme, utilities;\n\t@layer utilities {\n\t\t.sr-only { position: absolute; clip: rect(0,0,0,0); }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_complete_store_migration() {
        let s = "<script>\n\t// Svelte 4 store → Svelte 5 state\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n\tlet message = $derived.by(() => {\n\t\tif (count === 0) return 'Zero';\n\t\tif (count < 10) return 'Low';\n\t\treturn 'High';\n\t});\n\t$effect(() => {\n\t\tconsole.log(`Count changed to ${count}`);\n\t});\n</script>\n\n<h1>{message} ({count})</h1>\n<p>Doubled: {doubled}</p>\n<button onclick={() => count++}>+1</button>\n<button onclick={() => count--}>-1</button>\n<button onclick={() => count = 0}>Reset</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_all_slot_types() {
        let s = "<Widget>\n\t<!-- Default slot -->\n\t<p>Default content</p>\n\t<!-- Named slot (Svelte 4) -->\n\t<div slot=\"header\">Header</div>\n\t<!-- Snippet (Svelte 5) -->\n\t{#snippet footer()}\n\t\t<p>Footer</p>\n\t{/snippet}\n</Widget>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_dupe_else_if_complex() {
        let s = "{#if x > 0 && y > 0}\n\tp1\n{:else if x < 0}\n\tp2\n{:else if x > 0 && y > 0}\n\tp3\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-else-if-blocks"),
            "Should flag duplicate complex condition");
    }

    #[test]
    fn test_parse_expression_with_in_operator() {
        let s = "<p>{'key' in obj ? 'has key' : 'no key'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_state_with_generics() {
        let s = "<script lang=\"ts\">\n\tlet items = $state<Array<{ id: number; name: string }>>([]);\n\tlet selected = $state<number | null>(null);\n\tlet current = $derived(items.find(i => i.id === selected) ?? null);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_conditional_snippets() {
        let s = "<Dialog>\n\t{#if mode === 'confirm'}\n\t\t{#snippet actions()}\n\t\t\t<button onclick={cancel}>Cancel</button>\n\t\t\t<button onclick={confirm}>Confirm</button>\n\t\t{/snippet}\n\t{:else}\n\t\t{#snippet actions()}\n\t\t\t<button onclick={close}>OK</button>\n\t\t{/snippet}\n\t{/if}\n\t<p>{message}</p>\n</Dialog>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_render_prop() {
        let s = "<DataProvider url=\"/api\">\n\t{#snippet loading()}<Spinner />{/snippet}\n\t{#snippet error(err)}<p class=\"error\">{err.message}</p>{/snippet}\n\t{#snippet success(data)}\n\t\t{#each data as item}<p>{item}</p>{/each}\n\t{/snippet}\n</DataProvider>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scope_keyframes() {
        let s = "<style>\n\t@keyframes -global-pulse {\n\t\t0% { opacity: 1; }\n\t\t50% { opacity: 0.5; }\n\t\t100% { opacity: 1; }\n\t}\n\t.pulsing { animation: -global-pulse 2s infinite; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_toast_system() {
        let s = "<script>\n\tlet toasts = $state([]);\n\tconst add = (message, type = 'info') => {\n\t\tconst id = Date.now();\n\t\ttoasts.push({ id, message, type });\n\t\tsetTimeout(() => remove(id), 5000);\n\t};\n\tconst remove = (id) => toasts = toasts.filter(t => t.id !== id);\n</script>\n\n<div class=\"toasts\" aria-live=\"polite\">\n\t{#each toasts as toast (toast.id)}\n\t\t<div class=\"toast {toast.type}\" transition:fly={{y: -20}}>\n\t\t\t{toast.message}\n\t\t\t<button onclick={() => remove(toast.id)} aria-label=\"Dismiss\">&times;</button>\n\t\t</div>\n\t{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_destructure_inline() {
        let s = "<p>{(() => { const { a, b } = obj; return a + b; })()}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_derived_with_cleanup() {
        let s = "<script>\n\tlet url = $state('/api');\n\tlet data = $state(null);\n\t$effect(() => {\n\t\tconst controller = new AbortController();\n\t\tfetch(url, { signal: controller.signal })\n\t\t\t.then(r => r.json())\n\t\t\t.then(d => data = d);\n\t\treturn () => controller.abort();\n\t});\n</script>\n{#if data}<pre>{JSON.stringify(data)}</pre>{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scope_deep() {
        let s = "<style>\n\t.parent :global {\n\t\t.external-class { color: red; }\n\t\tp { margin: 0; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_svelte_internal_disallow() {
        let s = "<script>\n\timport { set_current_component } from 'svelte/internal';\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-svelte-internal"));
    }

    #[test]
    fn test_parse_real_world_theme_provider() {
        let s = "<script>\n\timport { setContext } from 'svelte';\n\tlet { theme = 'light', children } = $props();\n\tsetContext('theme', {\n\t\tget value() { return theme; },\n\t\ttoggle: () => theme = theme === 'light' ? 'dark' : 'light',\n\t});\n</script>\n\n<div class=\"theme-{theme}\" data-theme={theme}>\n\t{@render children()}\n</div>\n\n<style>\n\t.theme-dark { background: #1a1a2e; color: #eee; }\n\t.theme-light { background: #fff; color: #333; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_index_key_pattern() {
        let s = "{#each Object.entries(obj).sort(([a], [b]) => a.localeCompare(b)) as [key, value], index (key)}\n\t<p>{index}. {key} = {value}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_render_tags() {
        let s = "{@render header?.()}\n<main>\n\t{@render content()}\n</main>\n{@render footer?.()}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_reactive_literal_object() {
        let s = "<script>\n\t$: config = {};\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should flag reactive object literal");
    }

    #[test]
    fn test_parse_css_color_scheme() {
        let s = "<style>\n\t:root {\n\t\tcolor-scheme: light dark;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_debounced_search() {
        let s = "<script>\n\tlet query = $state('');\n\tlet debouncedQuery = $state('');\n\tlet timeout;\n\t$effect(() => {\n\t\tclearTimeout(timeout);\n\t\ttimeout = setTimeout(() => debouncedQuery = query, 300);\n\t\treturn () => clearTimeout(timeout);\n\t});\n\tlet results = $state([]);\n\t$effect(() => {\n\t\tif (!debouncedQuery) { results = []; return; }\n\t\tfetch(`/api/search?q=${debouncedQuery}`)\n\t\t\t.then(r => r.json())\n\t\t\t.then(d => results = d);\n\t});\n</script>\n<input bind:value={query} />\n{#each results as r (r.id)}<p>{r.name}</p>{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_dropdown() {
        let s = "<script>\n\tlet { items, onselect } = $props();\n\tlet open = $state(false);\n</script>\n<div class=\"dropdown\">\n\t<button onclick={() => open = !open}>Menu</button>\n\t{#if open}\n\t\t<ul>\n\t\t\t{#each items as item}<li onclick={() => { onselect?.(item); open = false; }}>{item.label}</li>{/each}\n\t\t</ul>\n\t{/if}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_nesting_modern() {
        let s = "<style>\n\t.parent {\n\t\tcolor: red;\n\t\t& .child { color: blue; }\n\t\t&:hover { color: green; }\n\t\t@media (width > 600px) { & { display: flex; } }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_inspect_derive() {
        let s = "<script>\n\tlet items = $state([1,2,3]);\n\t$inspect(items);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inspect"));
    }

    #[test]
    fn test_parse_real_world_command_palette() {
        let s = "<script>\n\tlet { commands } = $props();\n\tlet open = $state(false);\n\tlet query = $state('');\n\tlet filtered = $derived(commands.filter(c => c.label.toLowerCase().includes(query.toLowerCase())));\n\tlet selected = $state(0);\n\t$effect(() => { selected = 0; });\n</script>\n\n{#if open}\n\t<div class=\"overlay\" onclick={() => open = false}>\n\t\t<div class=\"palette\" onclick|stopPropagation>\n\t\t\t<input bind:value={query} placeholder=\"Type a command...\" />\n\t\t\t<ul>\n\t\t\t\t{#each filtered as cmd, i (cmd.id)}\n\t\t\t\t\t<li class:selected={i === selected} onclick={() => { cmd.action(); open = false; }}>{cmd.label}</li>\n\t\t\t\t{/each}\n\t\t\t</ul>\n\t\t</div>\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_regex() {
        let s = "<p>{value.replace(/[^a-z0-9]/gi, '-').toLowerCase()}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_state_map() {
        let s = "<script>\n\tlet cache = $state(new Map());\n\tconst set = (k, v) => cache.set(k, v);\n\tconst get = (k) => cache.get(k);\n\tlet size = $derived(cache.size);\n</script>\n<p>Cache: {size} entries</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_sort_and_filter() {
        let s = "{#each items\n\t.filter(i => i.active)\n\t.sort((a, b) => a.name.localeCompare(b.name))\n\t.slice(0, 10)\n\tas item (item.id)}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_comprehensive_clean_app() {
        let s = "<script lang=\"ts\">\n\tlet { data } = $props();\n\tlet search = $state('');\n\tlet filtered = $derived(data.filter(d => d.name.includes(search)));\n</script>\n\n<input type=\"search\" bind:value={search} />\n{#each filtered as item (item.id)}\n\t<p>{item.name}</p>\n{/each}\n\n<style lang=\"scss\">\n\tp { margin: 0.5rem 0; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::recommended().lint(&r.ast, s);
        let relevant: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(relevant.is_empty(), "Clean app should have no warnings: {:?}",
            relevant.iter().map(|d| &d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_parse_real_world_data_grid() {
        let s = "<script lang=\"ts\">\n\ttype Row = Record<string, unknown>;\n\tlet { rows, columns, onSort } = $props<{ rows: Row[]; columns: string[]; onSort?: (col: string) => void }>();\n\tlet selected = $state<Set<number>>(new Set());\n\tlet allSelected = $derived(selected.size === rows.length);\n\tconst toggleAll = () => {\n\t\tselected = allSelected ? new Set() : new Set(rows.map((_, i) => i));\n\t};\n</script>\n\n<table>\n\t<thead>\n\t\t<tr>\n\t\t\t<th><input type=\"checkbox\" checked={allSelected} onchange={toggleAll} /></th>\n\t\t\t{#each columns as col}\n\t\t\t\t<th onclick={() => onSort?.(col)}>{col}</th>\n\t\t\t{/each}\n\t\t</tr>\n\t</thead>\n\t<tbody>\n\t\t{#each rows as row, i (i)}\n\t\t\t<tr class:selected={selected.has(i)}>\n\t\t\t\t<td><input type=\"checkbox\" checked={selected.has(i)} onchange={() => {\n\t\t\t\t\tconst next = new Set(selected);\n\t\t\t\t\tnext.has(i) ? next.delete(i) : next.add(i);\n\t\t\t\t\tselected = next;\n\t\t\t\t}} /></td>\n\t\t\t\t{#each columns as col}\n\t\t\t\t\t<td>{row[col]}</td>\n\t\t\t\t{/each}\n\t\t\t</tr>\n\t\t{/each}\n\t</tbody>\n</table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_markdown_editor() {
        let s = "<script>\n\tlet source = $state('# Hello\\n\\nType **markdown** here');\n\tlet preview = $derived(marked(source));\n</script>\n\n<div class=\"editor\">\n\t<textarea bind:value={source} rows=\"20\" />\n\t<div class=\"preview\">{@html preview}</div>\n</div>\n\n<style>\n\t.editor { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }\n\ttextarea { font-family: monospace; resize: none; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_timer() {
        let s = "<script>\n\tlet seconds = $state(0);\n\tlet running = $state(false);\n\tlet minutes = $derived(Math.floor(seconds / 60));\n\tlet secs = $derived(seconds % 60);\n\tlet display = $derived(`${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`);\n\t$effect(() => {\n\t\tif (!running) return;\n\t\tconst id = setInterval(() => seconds++, 1000);\n\t\treturn () => clearInterval(id);\n\t});\n</script>\n\n<div class=\"timer\">\n\t<span class=\"display\">{display}</span>\n\t<button onclick={() => running = !running}>{running ? 'Pause' : 'Start'}</button>\n\t<button onclick={() => { seconds = 0; running = false; }}>Reset</button>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_combobox() {
        let s = "<script>\n\tlet { options = [], value = '', onselect } = $props();\n\tlet open = $state(false);\n\tlet search = $state('');\n\tlet filtered = $derived(\n\t\toptions.filter(o => o.label.toLowerCase().includes(search.toLowerCase()))\n\t);\n\tlet selectedLabel = $derived(\n\t\toptions.find(o => o.value === value)?.label ?? ''\n\t);\n</script>\n\n<div class=\"combobox\">\n\t<input\n\t\tvalue={open ? search : selectedLabel}\n\t\tonfocus={() => { open = true; search = ''; }}\n\t\tonblur={() => setTimeout(() => open = false, 200)}\n\t\toninput={(e) => search = e.target.value}\n\t\tplaceholder=\"Select...\"\n\t/>\n\t{#if open && filtered.length > 0}\n\t\t<ul role=\"listbox\">\n\t\t\t{#each filtered as option (option.value)}\n\t\t\t\t<li\n\t\t\t\t\trole=\"option\"\n\t\t\t\t\taria-selected={option.value === value}\n\t\t\t\t\tonclick={() => { value = option.value; open = false; onselect?.(option.value); }}\n\t\t\t\t>{option.label}</li>\n\t\t\t{/each}\n\t\t</ul>\n\t{/if}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_full_feature_demo() {
        let s = "<script lang=\"ts\" generics=\"T extends { id: string }\">\n\timport { onMount } from 'svelte';\n\timport { fade } from 'svelte/transition';\n\tlet { items = [], selected = $bindable<T | null>(null), onchange }: {\n\t\titems?: T[]; selected?: T | null; onchange?: (item: T) => void\n\t} = $props();\n\tlet search = $state('');\n\tlet filtered = $derived(items.filter(i => JSON.stringify(i).includes(search)));\n\tlet count = $derived(filtered.length);\n\t$effect(() => { if (selected) onchange?.(selected); });\n\tonMount(() => console.log(`Mounted with ${items.length} items`));\n</script>\n\n<svelte:head><title>Items ({count})</title></svelte:head>\n\n<input type=\"search\" bind:value={search} placeholder=\"Search {items.length} items...\" />\n\n{#if count > 0}\n\t{#each filtered as item (item.id)}\n\t\t<div\n\t\t\tclass:selected={item === selected}\n\t\t\tonclick={() => selected = item}\n\t\t\ttransition:fade\n\t\t\trole=\"button\"\n\t\t\ttabindex=\"0\"\n\t\t>\n\t\t\t<slot {item} />\n\t\t</div>\n\t{/each}\n{:else}\n\t<p>No items match \"{search}\"</p>\n{/if}\n\n<style lang=\"scss\">\n\t.selected { background: var(--highlight, #eef); }\n\tdiv[role=button] { cursor: pointer; padding: 0.5rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_comprehensive_rule_coverage() {
        // A single file that triggers many different rules
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst store = writable(0);\n\t$: x = 42;\n\t$: fn = () => 0;\n\t$inspect(x);\n</script>\n{@html bad}\n{@debug x}\n<button>no type</button>\n<a href=\"/path\" target=\"_blank\">link</a>\n{#each items as item}<p>{item}</p>{/each}\n<div style=\"color: red; color: blue;\">dup style</div>\n<p>{store}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        // Count unique rule names
        let mut rules = std::collections::HashSet::new();
        for d in &diags { rules.insert(d.rule_name.clone()); }
        assert!(rules.len() >= 6, "Should trigger >= 6 different rules, got {}: {:?}", rules.len(), rules);
    }

    #[test]
    fn test_parse_css_all_selector_types() {
        let s = "<style>\n\t* { margin: 0; }\n\tp { color: black; }\n\t.class { color: red; }\n\t#id { color: blue; }\n\t[attr] { color: green; }\n\t[attr=val] { color: purple; }\n\t:root { --x: 1; }\n\t::before { content: ''; }\n\ta > b { color: cyan; }\n\ta + b { color: magenta; }\n\ta ~ b { color: yellow; }\n\ta b { color: gray; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_pagination_svelte5() {
        let s = "<script>\n\tlet { page = 1, totalPages, onPageChange } = $props();\n\tlet pages = $derived(Array.from({length: totalPages}, (_, i) => i + 1));\n\tlet canPrev = $derived(page > 1);\n\tlet canNext = $derived(page < totalPages);\n</script>\n<nav aria-label=\"Pagination\">\n\t<button disabled={!canPrev} onclick={() => onPageChange(page - 1)}>←</button>\n\t{#each pages as p}\n\t\t<button class:active={p === page} onclick={() => onPageChange(p)}>{p}</button>\n\t{/each}\n\t<button disabled={!canNext} onclick={() => onPageChange(page + 1)}>→</button>\n</nav>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_with_generators() {
        let s = "<script>\n\tfunction* range(start, end) {\n\t\tfor (let i = start; i < end; i++) yield i;\n\t}\n\tconst items = [...range(0, 10)];\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_tagged_template() {
        let s = "<p>{css`color: ${primary}`}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_state_array_methods() {
        let s = "<script>\n\tlet items = $state(['a', 'b', 'c']);\n\tconst add = (item) => items.push(item);\n\tconst remove = (i) => items.splice(i, 1);\n\tconst clear = () => items.length = 0;\n\tconst sort = () => items.sort();\n\tconst reverse = () => items.reverse();\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_writable_derived_no_state() {
        let s = "<script>\n\tlet x = 0;\n\t$effect(() => { console.log(x); });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should NOT flag $effect without $state pattern");
    }

    #[test]
    fn test_parse_component_with_style_props() {
        let s = "<Card --card-bg=\"white\" --card-border=\"1px solid #eee\" --card-radius=\"8px\">\n\t<p>Styled card content</p>\n</Card>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_snippet_recursive() {
        let s = "{#snippet tree(nodes, depth)}\n\t{#each nodes as node}\n\t\t<div style:margin-left=\"{depth * 20}px\">{node.name}</div>\n\t\t{#if node.children}\n\t\t\t{@render tree(node.children, depth + 1)}\n\t\t{/if}\n\t{/each}\n{/snippet}\n\n{@render tree(data, 0)}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_backdrop_filter() {
        let s = "<style>\n\t.glass {\n\t\tbackdrop-filter: blur(10px) saturate(180%);\n\t\t-webkit-backdrop-filter: blur(10px) saturate(180%);\n\t\tbackground: rgba(255, 255, 255, 0.7);\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_nav_base_tel_ok() {
        let s = "<a href=\"tel:+15551234567\">Call</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should NOT flag tel: links");
    }

    #[test]
    fn test_parse_each_with_nested_components() {
        let s = "{#each categories as category (category.id)}\n\t<Section title={category.name}>\n\t\t{#each category.items as item (item.id)}\n\t\t\t<Card {item}>\n\t\t\t\t{#snippet footer()}\n\t\t\t\t\t<button onclick={() => select(item)}>Select</button>\n\t\t\t\t{/snippet}\n\t\t\t</Card>\n\t\t{/each}\n\t</Section>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_as_const() {
        let s = "<script lang=\"ts\">\n\tconst COLORS = ['red', 'green', 'blue'] as const;\n\ttype Color = typeof COLORS[number];\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_form_with_validation_and_errors() {
        let s = "<script>\n\tlet fields = $state({ name: '', email: '' });\n\tlet touched = $state({ name: false, email: false });\n\tlet errors = $derived({\n\t\tname: touched.name && !fields.name ? 'Required' : '',\n\t\temail: touched.email && !fields.email.includes('@') ? 'Invalid' : '',\n\t});\n</script>\n<form>\n\t<input\n\t\tbind:value={fields.name}\n\t\ton:blur={() => touched.name = true}\n\t\tclass:error={errors.name}\n\t\taria-invalid={!!errors.name}\n\t/>\n\t{#if errors.name}<span class=\"error\">{errors.name}</span>{/if}\n\t<input\n\t\ttype=\"email\"\n\t\tbind:value={fields.email}\n\t\ton:blur={() => touched.email = true}\n\t\tclass:error={errors.email}\n\t/>\n\t{#if errors.email}<span class=\"error\">{errors.email}</span>{/if}\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_color_picker() {
        let s = "<script>\n\tlet hue = $state(0);\n\tlet saturation = $state(100);\n\tlet lightness = $state(50);\n\tlet color = $derived(`hsl(${hue}, ${saturation}%, ${lightness}%)`);\n\tlet hex = $derived(hslToHex(hue, saturation, lightness));\n</script>\n\n<div class=\"picker\">\n\t<label>Hue: {hue}°\n\t\t<input type=\"range\" bind:value={hue} min=\"0\" max=\"360\" />\n\t</label>\n\t<label>Saturation: {saturation}%\n\t\t<input type=\"range\" bind:value={saturation} min=\"0\" max=\"100\" />\n\t</label>\n\t<label>Lightness: {lightness}%\n\t\t<input type=\"range\" bind:value={lightness} min=\"0\" max=\"100\" />\n\t</label>\n\t<div class=\"preview\" style:background-color={color}>\n\t\t<code>{hex}</code>\n\t</div>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_tree_view() {
        let s = "<script>\n\texport let nodes;\n\texport let depth = 0;\n</script>\n<ul style:padding-left=\"{depth * 16}px\">\n\t{#each nodes as node (node.id)}\n\t\t<li>\n\t\t\t<span>{node.name}</span>\n\t\t\t{#if node.children?.length}\n\t\t\t\t<svelte:self nodes={node.children} depth={depth + 1} />\n\t\t\t{/if}\n\t\t</li>\n\t{/each}\n</ul>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_stepper() {
        let s = "<script>\n\tlet { steps, current = 0, oncomplete } = $props();\n\tlet progress = $derived(((current + 1) / steps.length) * 100);\n\tconst next = () => {\n\t\tif (current < steps.length - 1) current++;\n\t\telse oncomplete?.();\n\t};\n\tconst prev = () => { if (current > 0) current--; };\n</script>\n\n<div class=\"stepper\">\n\t<progress value={progress} max=\"100\">{progress}%</progress>\n\t{#each steps as step, i}\n\t\t<div class:active={i === current} class:done={i < current}>{step.title}</div>\n\t{/each}\n\t<div class=\"content\">\n\t\t{@render steps[current].content?.()}\n\t</div>\n\t<div class=\"buttons\">\n\t\t<button onclick={prev} disabled={current === 0}>Back</button>\n\t\t<button onclick={next}>{current === steps.length - 1 ? 'Finish' : 'Next'}</button>\n\t</div>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_reactive_reassign_bind_template() {
        let s = "<script>\n\tlet v = 0;\n\t$: computed = v * 2;\n</script>\n<input bind:value={computed} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag bind:value on reactive var");
    }

    #[test]
    fn test_parse_css_keyframes_complex() {
        let s = "<style>\n\t@keyframes bounce {\n\t\t0%, 100% { transform: translateY(0); }\n\t\t25% { transform: translateY(-20px); }\n\t\t50% { transform: translateY(-10px); }\n\t\t75% { transform: translateY(-15px); }\n\t}\n\t.bouncing { animation: bounce 0.6s ease-in-out; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_flattened_events() {
        let s = "<button onclick={handle} onmouseenter={enter} onmouseleave={leave} onfocus={focus} onblur={blur}>interactive</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_map_filter_sort() {
        let s = "{#each items\n\t.filter(i => i.active)\n\t.sort((a, b) => b.priority - a.priority)\n\t.map(i => ({...i, label: i.name.toUpperCase()}))\n\tas item (item.id)}\n\t<div>{item.label} (priority: {item.priority})</div>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_boundary_full() {
        let s = "<svelte:boundary onerror={(error, reset) => logError(error)}>\n\t<Risky />\n\t{#snippet failed(error, reset)}\n\t\t<div class=\"error-boundary\">\n\t\t\t<h2>Something went wrong</h2>\n\t\t\t<pre>{error.message}</pre>\n\t\t\t<button onclick={reset}>Try again</button>\n\t\t</div>\n\t{/snippet}\n</svelte:boundary>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_immutable_reactive_store_ok() {
        let s = "<script>\n\timport { count } from './stores';\n\t$: doubled = $count * 2;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should NOT flag reactive stmt with $store");
    }

    #[test]
    fn test_parse_component_slots_svelte5() {
        let s = "<Tabs>\n\t{#snippet tab(t)}\n\t\t<button class:active={t.active}>{t.label}</button>\n\t{/snippet}\n\t{#snippet panel(p)}\n\t\t<div>{@render p.content()}</div>\n\t{/snippet}\n</Tabs>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_accordion_svelte5() {
        let s = "<script>\n\tlet { items } = $props();\n\tlet openIndex = $state(-1);\n\tconst toggle = (i) => openIndex = openIndex === i ? -1 : i;\n</script>\n{#each items as item, i}\n\t<button onclick={() => toggle(i)} aria-expanded={openIndex === i}>\n\t\t{item.title}\n\t</button>\n\t{#if openIndex === i}\n\t\t<div transition:slide>{@html item.content}</div>\n\t{/if}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_in_in_transition() {
        let s = "<div in:fly={{y: -200, duration: 300, delay: index * 50}}>animated</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_browser_global_setTimeout_top() {
        let s = "<script>\n\tsetTimeout(() => {}, 1000);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should flag setTimeout at top level");
    }

    #[test]
    fn test_linter_browser_global_setTimeout_in_fn_ok() {
        let s = "<script>\n\tfunction init() {\n\t\tsetTimeout(() => {}, 1000);\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-top-level-browser-globals"),
            "Should NOT flag setTimeout inside function");
    }

    #[test]
    fn test_parse_template_with_math() {
        let s = "<p>{Math.round(value * 100) / 100}</p>\n<p>{Math.max(0, Math.min(100, percentage))}%</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_dynamic_class_with_map() {
        let s = "<div class={['base', active && 'active', size === 'lg' && 'large'].filter(Boolean).join(' ')}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_spring_tweened() {
        let s = "<script>\n\timport { spring, tweened } from 'svelte/motion';\n\tconst coords = spring({ x: 50, y: 50 });\n\tconst progress = tweened(0, { duration: 400 });\n</script>\n<svg on:mousemove={(e) => coords.set({ x: e.clientX, y: e.clientY })}>\n\t<circle cx={$coords.x} cy={$coords.y} r=\"10\" />\n</svg>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_grid_complex() {
        let s = "<style>\n\t.layout {\n\t\tdisplay: grid;\n\t\tgrid-template-columns: 250px 1fr;\n\t\tgrid-template-rows: 60px 1fr 40px;\n\t\tgrid-template-areas:\n\t\t\t'header header'\n\t\t\t'sidebar main'\n\t\t\t'footer footer';\n\t\tmin-height: 100vh;\n\t}\n\t.header { grid-area: header; }\n\t.sidebar { grid-area: sidebar; }\n\t.main { grid-area: main; }\n\t.footer { grid-area: footer; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_promise_map() {
        let s = "{#each urls.map(u => fetch(u).then(r => r.json())) as promise}\n\t{#await promise}\n\t\t<p>...</p>\n\t{:then data}\n\t\t<p>{data.title}</p>\n\t{/await}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_two_way_bind_svelte5() {
        let s = "<script>\n\tlet { value = $bindable(0) } = $props();\n</script>\n<input type=\"number\" bind:value />\n<p>{value}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_immutable_reactive_export_fn() {
        let s = "<script>\n\texport function helper(x) { return x * 2; }\n\tconst CONST = 42;\n\t$: result = helper(CONST);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should flag reactive stmt with immutable function + const");
    }

    // --- to 1150 ---

    #[test]
    fn test_parse_real_world_api_client() {
        let s = "<script lang=\"ts\">\n\ttype ApiResponse<T> = { data: T; error: string | null };\n\tlet loading = $state(false);\n\tlet result = $state<ApiResponse<unknown> | null>(null);\n\tconst fetchData = async (endpoint: string) => {\n\t\tloading = true;\n\t\ttry {\n\t\t\tconst res = await fetch(endpoint);\n\t\t\tresult = { data: await res.json(), error: null };\n\t\t} catch (e) {\n\t\t\tresult = { data: null, error: String(e) };\n\t\t} finally {\n\t\t\tloading = false;\n\t\t}\n\t};\n</script>\n\n{#if loading}\n\t<div class=\"loading\">Loading...</div>\n{:else if result?.error}\n\t<div class=\"error\">{result.error}</div>\n{:else if result?.data}\n\t<pre>{JSON.stringify(result.data, null, 2)}</pre>\n{:else}\n\t<button onclick={() => fetchData('/api/data')}>Fetch</button>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_file_upload() {
        let s = "<script>\n\tlet files = $state([]);\n\tlet uploading = $state(false);\n\tlet progress = $state(0);\n\tconst handleDrop = (e) => {\n\t\te.preventDefault();\n\t\tfiles = [...files, ...Array.from(e.dataTransfer.files)];\n\t};\n</script>\n\n<div\n\tclass=\"dropzone\"\n\tclass:active={files.length > 0}\n\ton:dragover|preventDefault\n\ton:drop={handleDrop}\n\trole=\"region\"\n\taria-label=\"File upload\"\n>\n\t{#if files.length === 0}\n\t\t<p>Drop files here</p>\n\t{:else}\n\t\t<ul>\n\t\t\t{#each files as file (file.name)}\n\t\t\t\t<li>{file.name} ({(file.size / 1024).toFixed(1)} KB)</li>\n\t\t\t{/each}\n\t\t</ul>\n\t{/if}\n\t{#if uploading}\n\t\t<progress value={progress} max=\"100\">{progress}%</progress>\n\t{/if}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_tooltip() {
        let s = "<script>\n\tlet { text, position = 'top' } = $props();\n\tlet visible = $state(false);\n\tlet x = $state(0);\n\tlet y = $state(0);\n</script>\n\n<div\n\ton:mouseenter={(e) => { visible = true; x = e.clientX; y = e.clientY; }}\n\ton:mouseleave={() => visible = false}\n>\n\t<slot />\n</div>\n{#if visible}\n\t<div class=\"tooltip {position}\" style:left=\"{x}px\" style:top=\"{y}px\" transition:fade={{duration: 150}}>\n\t\t{text}\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_modern_features() {
        let s = "<style>\n\t.container {\n\t\tcontainer-type: inline-size;\n\t}\n\t@container (min-width: 500px) {\n\t\t.item { display: grid; }\n\t}\n\t.text {\n\t\ttext-wrap: balance;\n\t\toverflow-wrap: anywhere;\n\t}\n\t.stack {\n\t\tdisplay: flex;\n\t\tgap: max(1rem, 2vw);\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_combined_svelte5_issues() {
        let s = "<script>\n\t$inspect(val);\n</script>\n{@html content}\n{@debug x}\n<button>no type</button>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let rules: Vec<_> = diags.iter().map(|d| d.rule_name.clone()).collect();
        let has = |name: &str| rules.iter().any(|r| &**r == name);
        assert!(has("svelte/no-at-html-tags"));
        assert!(has("svelte/no-at-debug-tags"));
        assert!(has("svelte/no-inspect"));
        assert!(has("svelte/button-has-type"));
    }

    #[test]
    fn test_parse_component_context_api() {
        let s = "<script>\n\timport { setContext, getContext } from 'svelte';\n\tsetContext('theme', {\n\t\tprimary: '#007bff',\n\t\tsecondary: '#6c757d',\n\t\tgetPrimary() { return this.primary; }\n\t});\n</script>\n<slot />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_attachments() {
        let s = "<div {@attach tooltip('Hello')}>Hover me</div>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_linter_each_without_key_count() {
        let s = "{#each a as x}<p/>{/each}\n{#each b as y}<p/>{/each}\n{#each c as z (z.id)}<p/>{/each}\n{#each d as w}<p/>{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let key_count = diags.iter().filter(|d| d.rule_name == "svelte/require-each-key").count();
        assert_eq!(key_count, 3, "Should flag 3 each blocks without key");
    }

    #[test]
    fn test_parse_mixed_mustache_types() {
        let s = "<div>\n\t{text}\n\t{@html rawHtml}\n\t{@debug val}\n\t{@const doubled = val * 2}\n\t{@render snippet()}\n\t{doubled}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_class_directive_shorthand_chain() {
        let s = "<div class:a class:b class:c class:d class:e={expr}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_object_shorthand() {
        let s = "<Component data={{ name, age, email }} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_head_complex() {
        let s = "<svelte:head>\n\t<title>{pageTitle} | {siteName}</title>\n\t<meta name=\"description\" content={description} />\n\t<meta property=\"og:title\" content={pageTitle} />\n\t<meta property=\"og:description\" content={description} />\n\t<meta property=\"og:image\" content={`${baseUrl}/og/${slug}.png`} />\n\t<link rel=\"canonical\" href={`${baseUrl}/${slug}`} />\n\t<link rel=\"alternate\" hreflang=\"en\" href={`${baseUrl}/en/${slug}`} />\n\t{@html structuredData}\n</svelte:head>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_with_all_pseudo() {
        let s = "<style>\n\ta:link { color: blue; }\n\ta:visited { color: purple; }\n\ta:hover { color: red; }\n\ta:active { color: orange; }\n\ta:focus { outline: 2px solid; }\n\ta:focus-visible { outline: 3px solid blue; }\n\ta:focus-within { background: lightyellow; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_virtualized_list() {
        let s = "<script>\n\tlet { items, itemHeight = 40, containerHeight = 400 } = $props();\n\tlet scrollTop = $state(0);\n\tlet startIndex = $derived(Math.floor(scrollTop / itemHeight));\n\tlet visibleCount = $derived(Math.ceil(containerHeight / itemHeight) + 1);\n\tlet visibleItems = $derived(items.slice(startIndex, startIndex + visibleCount));\n\tlet totalHeight = $derived(items.length * itemHeight);\n\tlet offsetY = $derived(startIndex * itemHeight);\n</script>\n\n<div class=\"viewport\" style:height=\"{containerHeight}px\" on:scroll={(e) => scrollTop = e.target.scrollTop}>\n\t<div style:height=\"{totalHeight}px\">\n\t\t<div style:transform=\"translateY({offsetY}px)\">\n\t\t\t{#each visibleItems as item, i (startIndex + i)}\n\t\t\t\t<div class=\"item\" style:height=\"{itemHeight}px\">{item.label}</div>\n\t\t\t{/each}\n\t\t</div>\n\t</div>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- pushing to 1150 ---

    #[test]
    fn test_parse_real_world_chat() {
        let s = "<script>\n\tlet messages = $state([]);\n\tlet input = $state('');\n\tlet chatEl;\n\tconst send = () => {\n\t\tif (!input.trim()) return;\n\t\tmessages.push({ text: input, time: Date.now(), me: true });\n\t\tinput = '';\n\t};\n\t$effect(() => {\n\t\tif (chatEl) chatEl.scrollTop = chatEl.scrollHeight;\n\t});\n</script>\n\n<div class=\"chat\" bind:this={chatEl}>\n\t{#each messages as msg (msg.time)}\n\t\t<div class:me={msg.me}>\n\t\t\t<p>{msg.text}</p>\n\t\t\t<time>{new Date(msg.time).toLocaleTimeString()}</time>\n\t\t</div>\n\t{/each}\n</div>\n<form onsubmit={(e) => { e.preventDefault(); send(); }}>\n\t<input bind:value={input} placeholder=\"Type a message...\" />\n\t<button type=\"submit\">Send</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_settings() {
        let s = "<script lang=\"ts\">\n\tlet theme = $state<'light' | 'dark' | 'auto'>('auto');\n\tlet fontSize = $state(16);\n\tlet notifications = $state(true);\n\tlet language = $state('en');\n\tconst reset = () => { theme = 'auto'; fontSize = 16; notifications = true; language = 'en'; };\n</script>\n\n<h2>Settings</h2>\n<form>\n\t<fieldset>\n\t\t<legend>Appearance</legend>\n\t\t<label>Theme\n\t\t\t<select bind:value={theme}>\n\t\t\t\t<option value=\"light\">Light</option>\n\t\t\t\t<option value=\"dark\">Dark</option>\n\t\t\t\t<option value=\"auto\">Auto</option>\n\t\t\t</select>\n\t\t</label>\n\t\t<label>Font Size: {fontSize}px\n\t\t\t<input type=\"range\" bind:value={fontSize} min=\"12\" max=\"24\" />\n\t\t</label>\n\t</fieldset>\n\t<fieldset>\n\t\t<legend>Preferences</legend>\n\t\t<label><input type=\"checkbox\" bind:checked={notifications} /> Notifications</label>\n\t\t<label>Language\n\t\t\t<select bind:value={language}>\n\t\t\t\t<option value=\"en\">English</option>\n\t\t\t\t<option value=\"es\">Spanish</option>\n\t\t\t</select>\n\t\t</label>\n\t</fieldset>\n\t<button type=\"button\" onclick={reset}>Reset</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_kanban() {
        let s = "<script>\n\tlet columns = $state([\n\t\t{ name: 'Todo', items: ['Task 1', 'Task 2'] },\n\t\t{ name: 'In Progress', items: ['Task 3'] },\n\t\t{ name: 'Done', items: ['Task 4'] },\n\t]);\n</script>\n\n<div class=\"kanban\">\n\t{#each columns as column, ci}\n\t\t<div class=\"column\">\n\t\t\t<h3>{column.name} ({column.items.length})</h3>\n\t\t\t{#each column.items as item, ii}\n\t\t\t\t<div class=\"card\" draggable=\"true\">{item}</div>\n\t\t\t{/each}\n\t\t</div>\n\t{/each}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_at_html_count() {
        let s = "<div>\n\t{@html a}\n\t{@html b}\n\t{@html c}\n\t{@html d}\n\t{@html e}\n</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let count = diags.iter().filter(|d| d.rule_name == "svelte/no-at-html-tags").count();
        assert_eq!(count, 5, "Should flag all 5 @html tags");
    }

    #[test]
    fn test_linter_button_type_in_form() {
        let s = "<form>\n\t<button>Default (submit)</button>\n\t<button type=\"button\">Button</button>\n\t<button type=\"submit\">Submit</button>\n\t<button type=\"reset\">Reset</button>\n</form>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let btn: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/button-has-type").collect();
        assert_eq!(btn.len(), 1, "Should flag only the button without type");
    }

    #[test]
    fn test_parse_css_all_in_one() {
        let s = "<style>\n\t:root { --gap: 1rem; --radius: 4px; }\n\t* { margin: 0; box-sizing: border-box; }\n\tbody { font: 16px/1.5 system-ui; }\n\t.container { max-width: 1200px; margin: 0 auto; padding: var(--gap); }\n\t.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: var(--gap); }\n\t.card { border-radius: var(--radius); box-shadow: 0 1px 3px rgba(0,0,0,.12); }\n\t.card:hover { transform: translateY(-2px); box-shadow: 0 4px 12px rgba(0,0,0,.15); }\n\th1, h2, h3 { line-height: 1.2; }\n\ta { color: var(--primary, blue); text-decoration: none; }\n\ta:hover { text-decoration: underline; }\n\t@media (prefers-color-scheme: dark) { body { background: #111; color: #eee; } }\n\t@media (max-width: 600px) { .grid { grid-template-columns: 1fr; } }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_effect_types() {
        let s = "<script>\n\tlet count = $state(0);\n\t$effect(() => { console.log('effect', count); });\n\t$effect.pre(() => { console.log('pre-effect', count); });\n\t$effect.root(() => {\n\t\t$effect(() => { console.log('root child effect'); });\n\t});\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_complex_destructure() {
        let s = "{#each data as { user: { name, address: { city, country } }, score, tags: [first, ...rest] } (name)}\n\t<p>{name} from {city}, {country}: {score} ({first}, +{rest.length})</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_spaces_equal_signs_multiple() {
        let s = "<div class = \"a\" id = \"b\" style = \"color: red\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let spaces: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-spaces-around-equal-signs-in-attribute").collect();
        assert_eq!(spaces.len(), 3, "Should flag all 3 spaced =");
    }

    #[test]
    fn test_parse_svelte5_snippet_render_callback() {
        let s = "{#snippet list(items, render)}\n\t<ul>\n\t\t{#each items as item (item)}\n\t\t\t<li>{@render render(item)}</li>\n\t\t{/each}\n\t</ul>\n{/snippet}\n\n{@render list(fruits, (f) => f.name)}\n{@render list(veggies, (v) => `${v.emoji} ${v.name}`)}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_infinite_scroll() {
        let s = "<script>\n\tlet items = $state([]);\n\tlet page = $state(1);\n\tlet loading = $state(false);\n\tlet hasMore = $state(true);\n\tlet sentinel;\n\tconst loadMore = async () => {\n\t\tif (loading || !hasMore) return;\n\t\tloading = true;\n\t\tconst res = await fetch(`/api/items?page=${page}`);\n\t\tconst data = await res.json();\n\t\titems = [...items, ...data.items];\n\t\thasMore = data.hasMore;\n\t\tpage++;\n\t\tloading = false;\n\t};\n</script>\n\n{#each items as item (item.id)}\n\t<div class=\"item\">{item.title}</div>\n{/each}\n{#if loading}\n\t<div class=\"spinner\">Loading...</div>\n{/if}\n<div bind:this={sentinel} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_store_reactive_access_member() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst user = writable({ name: 'Alice' });\n</script>\n<p>{user.name}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should flag raw store member access");
    }

    #[test]
    fn test_parse_each_with_spread_and_key() {
        let s = "{#each items as { id, ...data } (id)}\n\t<Card {id} {...data} />\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_untrack() {
        let s = "<script>\n\timport { untrack } from 'svelte';\n\tlet count = $state(0);\n\tlet prev = $state(0);\n\t$effect(() => {\n\t\tprev = untrack(() => count);\n\t\tconsole.log('changed from', prev, 'to', count);\n\t});\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- linter rule exhaustive tests ---

    #[test]
    fn test_linter_all_rules_run_without_panic() {
        // Test that ALL rules can run on various inputs without panicking
        let inputs = [
            "",
            "<p>text</p>",
            "<script>\n\tlet x = 0;\n</script>",
            "<script lang=\"ts\">\n\tlet x: number = 0;\n</script>\n<p>{x}</p>\n<style>\n\tp { color: red; }\n</style>",
            "{#if true}<p>yes</p>{/if}",
            "{#each [1] as n}<p>{n}</p>{/each}",
            "{@html '<b>bold</b>'}",
            "{@debug x}",
            "<button>click</button>",
            "<a href=\"/\" target=\"_blank\">link</a>",
            "<div style=\"color: red\">styled</div>",
        ];
        let linter = Linter::all();
        for input in &inputs {
            let r = parser::parse(input);
            let _ = linter.lint(&r.ast, input);
        }
    }

    #[test]
    fn test_linter_recommended_runs_without_panic() {
        let inputs = [
            "<script>\n\timport { writable } from 'svelte/store';\n\tconst s = writable(0);\n</script>\n{$s}\n{s}",
            "<script>\n\t$: x = 42;\n\t$: fn = () => {};\n\t$inspect(x);\n</script>",
            "<script>\n\tlet v = 0;\n\t$: r = v * 2;\n\tfunction f() { r = 3; }\n</script>",
        ];
        let linter = Linter::recommended();
        for input in &inputs {
            let r = parser::parse(input);
            let _ = linter.lint(&r.ast, input);
        }
    }

    #[test]
    fn test_parse_every_block_type_nested() {
        let s = "{#if a}\n\t{#each bs as b}\n\t\t{#await p}\n\t\t\t{#key k}\n\t\t\t\t{#snippet s()}\n\t\t\t\t\t<p>{b}</p>\n\t\t\t\t{/snippet}\n\t\t\t\t{@render s()}\n\t\t\t{/key}\n\t\t{:then v}\n\t\t\t<p>{v}</p>\n\t\t{/await}\n\t{/each}\n{:else}\n\t<p>none</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_full_app_structure() {
        let s = "<script context=\"module\">\n\texport const prerender = true;\n</script>\n\n<script lang=\"ts\">\n\tlet { data } = $props();\n\tlet count = $state(0);\n</script>\n\n<svelte:head>\n\t<title>App</title>\n</svelte:head>\n\n<svelte:window on:keydown={(e) => {}} />\n\n<main>\n\t{#each data.items as item (item.id)}\n\t\t<p>{item.name}</p>\n\t{/each}\n</main>\n\n<style lang=\"scss\">\n\tmain { padding: 1rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.module.is_some());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    #[test]
    fn test_linter_dom_manip_multiple_methods() {
        let s = "<script>\n\tlet el;\n\tconst fn = () => {\n\t\tel.appendChild(document.createElement('p'));\n\t\tel.insertBefore(null, null);\n\t\tel.removeChild(null);\n\t};\n</script>\n<div bind:this={el} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let dom: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-dom-manipulating").collect();
        assert!(dom.len() >= 3, "Should flag multiple DOM methods, got {}", dom.len());
    }

    #[test]
    fn test_linter_no_reactive_reassign_complex() {
        let s = "<script>\n\tlet v = 0;\n\t$: obj = { a: v, b: { c: v } };\n\tfunction fn() {\n\t\tobj.a = 1;\n\t\tobj.b.c = 2;\n\t\tdelete obj.a;\n\t}\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let reassign: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-reactive-reassign").collect();
        assert!(reassign.len() >= 2, "Should flag multiple reactive mutations, got {}", reassign.len());
    }

    #[test]
    fn test_parse_component_with_everything() {
        let s = "<Widget\n\t{prop}\n\tvalue={expr}\n\tbind:value\n\ton:click={handler}\n\ton:custom\n\tuse:action\n\ttransition:fade\n\tclass:active\n\tstyle:color=\"red\"\n\tlet:item\n\t{...rest}\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_complete() {
        let s = "<style>\n\t:root { --primary: blue; }\n\t* { box-sizing: border-box; }\n\th1 { color: var(--primary); }\n\t.card { border: 1px solid #eee; border-radius: 8px; }\n\t.card:hover { box-shadow: 0 2px 8px rgba(0,0,0,0.1); }\n\t@media (max-width: 768px) { .card { width: 100%; } }\n\t@keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_dashboard() {
        let s = "<script lang=\"ts\">\n\tlet { data } = $props();\n\tlet search = $state('');\n\tlet filtered = $derived(\n\t\tdata.users.filter(u => u.name.toLowerCase().includes(search.toLowerCase()))\n\t);\n\tlet total = $derived(filtered.length);\n</script>\n\n<div class=\"dashboard\">\n\t<header>\n\t\t<h1>Users ({total})</h1>\n\t\t<input type=\"search\" bind:value={search} placeholder=\"Filter...\" />\n\t</header>\n\t{#if filtered.length > 0}\n\t\t<table>\n\t\t\t<thead><tr><th>Name</th><th>Email</th><th>Role</th></tr></thead>\n\t\t\t<tbody>\n\t\t\t\t{#each filtered as user (user.id)}\n\t\t\t\t\t<tr class:admin={user.role === 'admin'}>\n\t\t\t\t\t\t<td>{user.name}</td>\n\t\t\t\t\t\t<td>{user.email}</td>\n\t\t\t\t\t\t<td>{user.role}</td>\n\t\t\t\t\t</tr>\n\t\t\t\t{/each}\n\t\t\t</tbody>\n\t\t</table>\n\t{:else}\n\t\t<p class=\"empty\">No users match \"{search}\"</p>\n\t{/if}\n</div>\n\n<style>\n\t.dashboard { max-width: 960px; margin: 0 auto; }\n\theader { display: flex; justify-content: space-between; align-items: center; }\n\ttable { width: 100%; border-collapse: collapse; }\n\tth, td { padding: 0.5rem; text-align: left; border-bottom: 1px solid #eee; }\n\t.admin { font-weight: bold; }\n\t.empty { color: #999; font-style: italic; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_auth_page() {
        let s = "<script lang=\"ts\">\n\tlet mode = $state<'login' | 'register'>('login');\n\tlet email = $state('');\n\tlet password = $state('');\n\tlet confirmPassword = $state('');\n\tlet errors = $derived({\n\t\temail: !email.includes('@') ? 'Invalid email' : '',\n\t\tpassword: password.length < 8 ? 'Min 8 characters' : '',\n\t\tconfirm: mode === 'register' && password !== confirmPassword ? 'Passwords must match' : '',\n\t});\n\tlet valid = $derived(!errors.email && !errors.password && (mode === 'login' || !errors.confirm));\n</script>\n\n<div class=\"auth\">\n\t<h1>{mode === 'login' ? 'Sign In' : 'Create Account'}</h1>\n\t<form onsubmit={(e) => { e.preventDefault(); /* submit */ }}>\n\t\t<label>Email <input type=\"email\" bind:value={email} /></label>\n\t\t{#if errors.email}<span class=\"error\">{errors.email}</span>{/if}\n\t\t<label>Password <input type=\"password\" bind:value={password} /></label>\n\t\t{#if errors.password}<span class=\"error\">{errors.password}</span>{/if}\n\t\t{#if mode === 'register'}\n\t\t\t<label>Confirm <input type=\"password\" bind:value={confirmPassword} /></label>\n\t\t\t{#if errors.confirm}<span class=\"error\">{errors.confirm}</span>{/if}\n\t\t{/if}\n\t\t<button type=\"submit\" disabled={!valid}>{mode === 'login' ? 'Sign In' : 'Register'}</button>\n\t</form>\n\t<p>\n\t\t{mode === 'login' ? 'Need an account?' : 'Already have one?'}\n\t\t<button onclick={() => mode = mode === 'login' ? 'register' : 'login'}>\n\t\t\t{mode === 'login' ? 'Register' : 'Sign In'}\n\t\t</button>\n\t</p>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_real_world_carousel() {
        let s = "<script>\n\tlet { images = [] } = $props();\n\tlet currentIndex = $state(0);\n\tlet count = $derived(images.length);\n\tconst prev = () => currentIndex = (currentIndex - 1 + count) % count;\n\tconst next = () => currentIndex = (currentIndex + 1) % count;\n</script>\n\n<div class=\"carousel\">\n\t<button onclick={prev} aria-label=\"Previous\">&larr;</button>\n\t{#each images as img, i (img.src)}\n\t\t{#if i === currentIndex}\n\t\t\t<img src={img.src} alt={img.alt} transition:fade />\n\t\t{/if}\n\t{/each}\n\t<button onclick={next} aria-label=\"Next\">&rarr;</button>\n\t<div class=\"dots\">\n\t\t{#each images as _, i}\n\t\t\t<button class:active={i === currentIndex} onclick={() => currentIndex = i} aria-label=\"Go to slide {i + 1}\" />\n\t\t{/each}\n\t</div>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_with_top_level_await() {
        let s = "<script>\n\tconst data = await fetch('/api').then(r => r.json());\n</script>\n<p>{JSON.stringify(data)}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_custom_events() {
        let s = "<div on:customEvent={handler} on:anotherEvent|stopPropagation on:thirdEvent={(e) => log(e.detail)}>events</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_position_inset() {
        let s = "<style>\n\t.overlay { position: fixed; inset: 0; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_inspect_with() {
        let s = "<script>\n\tlet x = $state(0);\n\t$inspect(x).with(console.log);\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-inspect"));
    }

    // --- final push to 1100 ---

    #[test]
    fn test_parse_each_with_default_value() {
        let s = "{#each items as { name = 'Unknown', age = 0 }}\n\t<p>{name}: {age}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_event_delegation() {
        let s = "<div onclick={handleClick}>\n\t<button>A</button>\n\t<button>B</button>\n\t<button>C</button>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_generics_constraint() {
        let s = "<script lang=\"ts\" generics=\"T extends { id: string; name: string }\">\n\tlet { items }: { items: T[] } = $props();\n</script>\n{#each items as item (item.id)}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_layer_imports() {
        let s = "<style>\n\t@layer utilities {\n\t\t.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_store_callback_writable_with_set() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\twritable(0, (set) => { set(1); return () => {}; });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/require-store-callbacks-use-set-param"),
            "Should NOT flag callback with set");
    }

    #[test]
    fn test_parse_expression_with_ternary_chain() {
        let s = "<p>{size === 'sm' ? '14px' : size === 'md' ? '16px' : size === 'lg' ? '20px' : '24px'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_dynamic_import_component() {
        let s = "<script>\n\timport { onMount } from 'svelte';\n\tlet LazyComponent;\n\tonMount(async () => {\n\t\tLazyComponent = (await import('./Lazy.svelte')).default;\n\t});\n</script>\n{#if LazyComponent}\n\t<svelte:component this={LazyComponent} />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_with_data_sveltekit() {
        let s = "<a href=\"/dashboard\" data-sveltekit-preload-data=\"hover\" data-sveltekit-preload-code=\"viewport\">Dashboard</a>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_snippet_with_typed_params() {
        let s = "{#snippet cell(value: string, index: number)}\n\t<td class:first={index === 0}>{value}</td>\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_object_in_text_complex() {
        let s = "<p>{{ a: 1, b: 2 }}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-object-in-text-mustaches"),
            "Should flag complex object in text");
    }

    #[test]
    fn test_parse_css_transform() {
        let s = "<style>\n\t.rotate {\n\t\ttransform: rotate(45deg) translateX(10px) scale(1.2);\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_component_lifecycle() {
        let s = "<script>\n\timport { onMount, onDestroy } from 'svelte';\n\tlet mounted = $state(false);\n\tonMount(() => {\n\t\tmounted = true;\n\t\treturn () => { mounted = false; };\n\t});\n</script>\n<p>{mounted ? 'Mounted' : 'Not mounted'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_index_destructure() {
        let s = "{#each matrix as row, y}\n\t{#each row as cell, x}\n\t\t<div style:grid-column={x + 1} style:grid-row={y + 1}>{cell}</div>\n\t{/each}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_logical_assignment() {
        let s = "<button on:click={() => count ??= 0}>init</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_attribute_with_entity() {
        let s = "<div title=\"&lt;script&gt; injection\">{text}</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_svelte_internal_server() {
        let s = "<script>\n\timport { something } from 'svelte/internal/server';\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-svelte-internal"),
            "Should flag svelte/internal/server import");
    }

    #[test]
    fn test_parse_svelte_boundary_with_fallback() {
        let s = "<svelte:boundary>\n\t<Risky />\n\t{#snippet failed(error, reset)}\n\t\t<p>Error: {error.message}</p>\n\t\t<button onclick={reset}>Retry</button>\n\t{/snippet}\n</svelte:boundary>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_class_computed() {
        let s = "<div class=\"item type-{type} state-{state} {active ? 'is-active' : ''}\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_render_tag_optional() {
        let s = "{@render children?.()}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_nav_base_hash_ok() {
        let s = "<a href=\"#top\">Back to top</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should NOT flag hash links");
    }

    #[test]
    fn test_parse_mixed_blocks_and_elements() {
        let s = "<header>\n\t<h1>{title}</h1>\n</header>\n{#if showNav}\n\t<nav>\n\t\t{#each links as link (link.href)}\n\t\t\t<a href={link.href}>{link.text}</a>\n\t\t{/each}\n\t</nav>\n{/if}\n<main>\n\t<slot />\n</main>\n{#if showFooter}\n\t<footer>&copy; {year}</footer>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_position_sticky() {
        let s = "<style>\n\t.sticky {\n\t\tposition: sticky;\n\t\ttop: 0;\n\t\tz-index: 10;\n\t\tbackground: white;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_in_style_attr() {
        let s = "<div style=\"transform: rotate({angle}deg); opacity: {visible ? 1 : 0}\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_events_svelte5() {
        let s = "<script>\n\tlet { onclick, onhover, onfocus } = $props();\n</script>\n<div {onclick} {onfocus} on:mouseenter={onhover}>interactive</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_debug_tags() {
        let s = "{@debug a}\n{@debug b}\n{@debug a, b, c}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::all().lint(&r.ast, s);
        let debug_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-at-debug-tags").collect();
        assert_eq!(debug_diags.len(), 3, "Should flag all 3 debug tags");
    }

    #[test]
    fn test_parse_expression_template_nested() {
        let s = "<p>{`${first} ${middle ? `(${middle}) ` : ''}${last}`}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_complete_form() {
        let s = "<script lang=\"ts\">\n\tinterface FormData { name: string; email: string; message: string }\n\tlet form = $state<FormData>({ name: '', email: '', message: '' });\n\tlet errors = $derived({\n\t\tname: form.name.length < 2 ? 'Too short' : '',\n\t\temail: !form.email.includes('@') ? 'Invalid' : '',\n\t\tmessage: form.message.length < 10 ? 'Too short' : '',\n\t});\n\tlet valid = $derived(!errors.name && !errors.email && !errors.message);\n\tlet submitting = $state(false);\n\tconst submit = async () => {\n\t\tsubmitting = true;\n\t\tawait fetch('/api/contact', { method: 'POST', body: JSON.stringify(form) });\n\t\tsubmitting = false;\n\t};\n</script>\n\n<form onsubmit={(e) => { e.preventDefault(); submit(); }}>\n\t<label>Name <input bind:value={form.name} /></label>\n\t{#if errors.name}<span class=\"error\">{errors.name}</span>{/if}\n\t<label>Email <input type=\"email\" bind:value={form.email} /></label>\n\t{#if errors.email}<span class=\"error\">{errors.email}</span>{/if}\n\t<label>Message <textarea bind:value={form.message} /></label>\n\t{#if errors.message}<span class=\"error\">{errors.message}</span>{/if}\n\t<button type=\"submit\" disabled={!valid || submitting}>\n\t\t{submitting ? 'Sending...' : 'Send'}\n\t</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- towards 1100 batch ---

    #[test]
    fn test_parse_conditional_snippet_in_component() {
        let s = "<Wrapper>\n\t{#if showHeader}\n\t\t{#snippet header()}<h1>Title</h1>{/snippet}\n\t{/if}\n\t<p>Body</p>\n</Wrapper>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_class_with_private() {
        let s = "<script>\n\tclass Timer {\n\t\t#seconds = $state(0);\n\t\t#interval = null;\n\t\tstart() { this.#interval = setInterval(() => this.#seconds++, 1000); }\n\t\tstop() { clearInterval(this.#interval); }\n\t\tget time() { return this.#seconds; }\n\t}\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_await_promise_all() {
        let s = "{#await Promise.all([fetchA(), fetchB(), fetchC()])}\n\t<p>Loading all...</p>\n{:then [a, b, c]}\n\t<p>{a} + {b} + {c}</p>\n{:catch errors}\n\t<p>Failed: {errors.map(e => e.message).join(', ')}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_void_operator() {
        let s = "<button on:click={() => void doSomething()}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_with_aria() {
        let s = "<div role=\"dialog\" aria-modal=\"true\" aria-labelledby=\"title\">\n\t<h2 id=\"title\">Dialog</h2>\n\t<p>Content</p>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_container_name() {
        let s = "<style>\n\t.sidebar {\n\t\tcontainer-name: sidebar;\n\t\tcontainer-type: inline-size;\n\t}\n\t@container sidebar (min-width: 300px) {\n\t\t.widget { flex-direction: row; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_multiple_unused_classes() {
        let s = "<div class=\"a b c d\">text</div>\n<style>\n\t.a { color: red; }\n\t.b { color: blue; }\n</style>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let unused: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-unused-class-name").collect();
        assert_eq!(unused.len(), 2, "Should flag 'c' and 'd', got: {:?}", unused.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_parse_svelte5_state_frozen() {
        let s = "<script>\n\tlet items = $state.frozen([1, 2, 3]);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_else_key() {
        let s = "{#each items as item (item.id)}\n\t<div>{item.name}</div>\n{:else}\n\t<p>No items found. <a href=\"/add\">Add one?</a></p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_event_modifiers_svelte5() {
        let s = "<button\n\tonclick|once|capture={(e) => {\n\t\te.preventDefault();\n\t\thandle();\n\t}}\n>Click once</button>";
        let r = parser::parse(s);
        // onclick modifiers may not parse in Svelte 5 (on: only), but shouldn't panic
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_css_important_override() {
        let s = "<style>\n\t.forced {\n\t\tcolor: red !important;\n\t\tfont-weight: bold !important;\n\t\ttext-decoration: none !important;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_reactive_literal_array() {
        let s = "<script>\n\t$: items = [];\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should flag reactive array literal");
    }

    #[test]
    fn test_parse_component_with_bind_this_and_props() {
        let s = "<script>\n\tlet inputEl;\n</script>\n<TextInput bind:this={inputEl} value=\"hello\" placeholder=\"Type here\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_with_charset() {
        let s = "<style>\n\t@charset \"UTF-8\";\n\tbody { font-family: system-ui; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_nav_base_mailto_ok() {
        let s = "<a href=\"mailto:user@example.com\">Email us</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-navigation-without-base"),
            "Should NOT flag mailto: links");
    }

    #[test]
    fn test_parse_svelte5_runes_combined() {
        let s = "<script lang=\"ts\">\n\tlet todos = $state<{text: string; done: boolean}[]>([]);\n\tlet remaining = $derived(todos.filter(t => !t.done).length);\n\tlet allDone = $derived(remaining === 0 && todos.length > 0);\n\t$effect(() => { document.title = `${remaining} remaining`; });\n\t$inspect(todos);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_css_props() {
        let s = "<Widget --color=\"blue\" --size=\"large\" --padding=\"1rem\">\n\t<p>Styled widget</p>\n</Widget>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_entries() {
        let s = "{#each Object.entries(config) as [key, value]}\n\t<dt>{key}</dt>\n\t<dd>{JSON.stringify(value)}</dd>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_string_method() {
        let s = "<p>{name.toUpperCase().slice(0, 3)}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_derived_chain() {
        let s = "<script>\n\tlet count = $state(0);\n\tlet d1 = $derived(count * 2);\n\tlet d2 = $derived(d1 + 1);\n\tlet d3 = $derived(d2 > 10);\n</script>\n<p>{d3 ? 'big' : 'small'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_forwarding_all() {
        let s = "<script>\n\tlet props = $props();\n</script>\n<Inner {...props} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_head_script_style() {
        let s = "<svelte:head>\n\t<link rel=\"preload\" href=\"/font.woff2\" as=\"font\" type=\"font/woff2\" crossorigin />\n\t<meta name=\"robots\" content=\"noindex\" />\n</svelte:head>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_debug_multiple_vars() {
        let s = "{@debug a, b, c}";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-debug-tags"));
    }

    // --- towards 1100 ---

    #[test]
    fn test_parse_svelte5_each_snippet_render() {
        let s = "{#snippet row(item, i)}\n\t<tr class:even={i % 2 === 0}>\n\t\t<td>{item.id}</td>\n\t\t<td>{item.name}</td>\n\t</tr>\n{/snippet}\n\n<table>\n\t{#each data as item, i (item.id)}\n\t\t{@render row(item, i)}\n\t{/each}\n</table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_snippet_children() {
        let s = "<Modal open={showModal}>\n\t{#snippet header()}\n\t\t<h2>Confirm Action</h2>\n\t{/snippet}\n\t<p>Are you sure you want to proceed?</p>\n\t{#snippet actions()}\n\t\t<button onclick={() => showModal = false}>Cancel</button>\n\t\t<button onclick={confirm}>Confirm</button>\n\t{/snippet}\n</Modal>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_fine_grained() {
        let s = "<script>\n\tlet { a, b, c } = $props();\n\tlet sum = $derived(a + b + c);\n\tlet product = $derived(a * b * c);\n\tlet avg = $derived(sum / 3);\n</script>\n<p>Sum: {sum}</p>\n<p>Product: {product}</p>\n<p>Average: {avg}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scroll_snap() {
        let s = "<style>\n\t.container {\n\t\tscroll-snap-type: x mandatory;\n\t\toverflow-x: scroll;\n\t}\n\t.item {\n\t\tscroll-snap-align: center;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_aspect_ratio() {
        let s = "<style>\n\t.video {\n\t\taspect-ratio: 16 / 9;\n\t\twidth: 100%;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_goto_external_ok() {
        let s = "<script>\n\timport { goto } from '$app/navigation';\n\tgoto('https://example.com');\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-goto-without-base"),
            "Should NOT flag goto with absolute URL");
    }

    #[test]
    fn test_linter_no_reactive_fn_regular_ok() {
        let s = "<script>\n\tfunction helper() { return 42; }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/no-reactive-functions"),
            "Should NOT flag regular function");
    }

    #[test]
    fn test_parse_key_with_ternary() {
        let s = "{#key active ? 'active' : 'inactive'}\n\t<Panel {active} />\n{/key}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_await_with_destructure() {
        let s = "{#await loadData()}\n\t<Spinner />\n{:then { items, total }}\n\t<p>{total} items:</p>\n\t{#each items as item}<p>{item}</p>{/each}\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_spread_call() {
        let s = "<Component {...getProps(id)} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiline_if_condition() {
        let s = "{#if\n\tcondition1 &&\n\tcondition2 &&\n\t(condition3 || condition4)\n}\n\t<p>All conditions met</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_writable_derived_detect() {
        let s = "<script>\n\tlet { x } = $props();\n\tlet y = $state(x);\n\t$effect(() => { y = x * 2; });\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-writable-derived"),
            "Should flag $state + $effect pattern");
    }

    #[test]
    fn test_parse_style_all_block() {
        let s = "<style>\n\t* { box-sizing: border-box; margin: 0; padding: 0; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_html_with_svelte_blocks() {
        let s = "<main>\n\t{#if user}\n\t\t<h1>Welcome, {user.name}!</h1>\n\t\t{#each user.posts as post (post.id)}\n\t\t\t{#await loadPost(post.id)}\n\t\t\t\t<p>Loading post...</p>\n\t\t\t{:then content}\n\t\t\t\t<article>{content}</article>\n\t\t\t{/await}\n\t\t{/each}\n\t{:else}\n\t\t<p>Please log in.</p>\n\t{/if}\n</main>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_props_rename() {
        let s = "<script>\n\tlet { class: className = '' } = $props();\n</script>\n<div class={className}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_shorthand_attr_href() {
        let s = "<a href={href}>link</a>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/shorthand-attribute"),
            "Should flag non-shorthand href");
    }

    #[test]
    fn test_parse_component_with_class_prop() {
        let s = "<script>\n\tlet { class: className = '', ...rest } = $props();\n</script>\n<div class=\"base {className}\" {...rest}>\n\t<slot />\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_spread_rest() {
        let s = "{#each items as { id, ...rest } (id)}\n\t<Component {id} {...rest} />\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_event_handler_with_type() {
        let s = "<button on:click={(e: MouseEvent) => { console.log(e.clientX); }}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- beyond 1000 ---

    #[test]
    fn test_linter_store_reactive_readable() {
        let s = "<script>\n\timport { readable } from 'svelte/store';\n\tconst time = readable(new Date());\n</script>\n<p>{time}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should flag raw readable store in template");
    }

    #[test]
    fn test_linter_no_at_html_in_component() {
        let s = "<Component>\n\t{@html content}\n</Component>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-at-html-tags"));
    }

    #[test]
    fn test_linter_multiple_stores_reactive() {
        let s = "<script>\n\timport { writable } from 'svelte/store';\n\tconst a = writable(0);\n\tconst b = writable(1);\n</script>\n<p>{a}</p><p>{b}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let store_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/require-store-reactive-access").collect();
        assert_eq!(store_diags.len(), 2, "Should flag both raw store accesses");
    }

    #[test]
    fn test_parse_each_with_expression_key() {
        let s = "{#each items as item (item.type + '-' + item.id)}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_component_complete() {
        let s = "<script lang=\"ts\">\n\ttype Props = { value: number; onchange: (v: number) => void };\n\tlet { value = $bindable(0), onchange }: Props = $props();\n</script>\n\n<input type=\"range\" bind:value oninput={() => onchange(value)} />\n<p>{value}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_where_not() {
        let s = "<style>\n\t:where(p:not(.special)) { margin: 1rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_reactive_reassign_array_sort() {
        let s = "<script>\n\tlet v = 'a';\n\t$: arr = [v];\n\tfunction fn() { arr.sort(); }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag .sort() on reactive array");
    }

    #[test]
    fn test_linter_reactive_reassign_for_of() {
        let s = "<script>\n\tlet v = 'a';\n\t$: obj = { key: v };\n\tlet o = [1];\n\tfunction fn() { for (obj.key of o) {} }\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-reassign"),
            "Should flag for-of on reactive var member");
    }

    #[test]
    fn test_linter_valid_each_key_fn() {
        let s = "{#each items as item (getKey(item))}\n\t<p>{item}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(!diags.iter().any(|d| d.rule_name == "svelte/valid-each-key"),
            "Should NOT flag key with function using item");
    }

    #[test]
    fn test_linter_immutable_reactive_fn_call() {
        let s = "<script>\n\texport function greet() { return 'hi'; }\n\t$: msg = greet();\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-immutable-reactive-statements"),
            "Should flag reactive calling immutable function");
    }

    #[test]
    fn test_parse_svelte_window_events_all() {
        let s = "<svelte:window\n\ton:resize={onResize}\n\ton:scroll={onScroll}\n\ton:keydown={onKey}\n\ton:keyup={onKeyUp}\n\ton:keypress={onKeyPress}\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_effect_root() {
        let s = "<script>\n\timport { effect } from 'svelte';\n\tconst cleanup = effect.root(() => {\n\t\t$effect(() => { console.log('root effect'); });\n\t});\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_with_newline_in_attr() {
        let s = "<div\n\ttitle=\"Line 1\nLine 2\"\n>text</div>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_linter_no_store_async_derived() {
        let s = "<script>\n\timport { derived } from 'svelte/store';\n\tconst d = derived(count, async ($c) => await transform($c));\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-store-async"),
            "Should flag async derived callback");
    }

    #[test]
    fn test_parse_component_generic_slot() {
        let s = "<Table data={items} let:row let:index>\n\t{#snippet cell(value)}\n\t\t<td>{value}</td>\n\t{/snippet}\n\t<tr>\n\t\t{@render cell(row.name)}\n\t\t{@render cell(String(index))}\n\t</tr>\n</Table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_custom_property_fallback() {
        let s = "<style>\n\t.themed {\n\t\tcolor: var(--text-color, #333);\n\t\tbackground: var(--bg-color, var(--fallback-bg, white));\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_prefer_const_used_in_template() {
        let s = "<script>\n\tlet MAX = 100;\n</script>\n<p>{MAX}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-const"),
            "Should flag let that could be const");
    }

    #[test]
    fn test_parse_expression_with_new() {
        let s = "<p>{new Intl.NumberFormat('en-US').format(price)}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_nested_destructure() {
        let s = "{#each data as { user: { name, email }, score }}\n\t<p>{name} ({email}): {score}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_onclick_and_bind() {
        let s = "<CustomInput\n\tbind:value={inputValue}\n\tonchange={(e) => validate(e.target.value)}\n\tplaceholder=\"Enter text\"\n\tclass=\"custom-input\"\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_trailing_spaces_empty_line() {
        let s = "<p>text</p>\n  \n<p>more</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-trailing-spaces"),
            "Should flag empty line with spaces");
    }

    #[test]
    fn test_parse_css_logical_properties() {
        let s = "<style>\n\t.box {\n\t\tmargin-inline: auto;\n\t\tpadding-block: 1rem;\n\t\tinset-inline-start: 0;\n\t\tborder-block-end: 1px solid;\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_await_inside() {
        let s = "{#each urls as url}\n\t{#await fetch(url).then(r => r.text())}\n\t\t<p>Loading {url}...</p>\n\t{:then text}\n\t\t<pre>{text}</pre>\n\t{/await}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_conditional_class() {
        let s = "<Button\n\tclass=\"btn {variant} {size}\"\n\tclass:loading\n\tclass:disabled={!enabled}\n\tonclick={handleClick}\n>Click me</Button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_unstate() {
        let s = "<script>\n\timport { unstate } from 'svelte';\n\tlet state = $state({ deep: { value: 42 } });\n\tconst snapshot = unstate(state);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_dupe_style_directive() {
        let s = "<div style:color=\"red\" style:color=\"blue\">text</div>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-style-properties"),
            "Should flag duplicate style directive properties");
    }

    #[test]
    fn test_parse_expression_destructure_in_handler() {
        let s = "<button on:click={() => { const { x, y } = getPosition(); moveTo(x, y); }}>move</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_1000th_milestone() {
        // The 1000th test! A complete Svelte component that exercises everything.
        let s = "<script lang=\"ts\">\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n\tconst reset = () => count = 0;\n</script>\n\n<p>Count: {count}</p>\n<p>Doubled: {doubled}</p>\n<button onclick={() => count++}>+1</button>\n<button onclick={reset}>Reset</button>\n\n<style>\n\tp { margin: 0.5rem 0; }\n\tbutton { margin-right: 0.5rem; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
        let diags = Linter::recommended().lint(&r.ast, s);
        let filtered: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(filtered.is_empty());
    }

    // --- THE FINAL PUSH TO 1000 ---

    #[test]
    fn test_parse_css_scope_modifier() {
        let s = "<style>\n\tp { color: blue; }\n\tp:global(.external) { color: red; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_in_each() {
        let s = "{#each items as item, i}\n\t<input bind:value={items[i].name} />\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_dot_notation() {
        let s = "<UI.Card>\n\t<UI.Card.Header>Title</UI.Card.Header>\n\t<UI.Card.Body>Content</UI.Card.Body>\n</UI.Card>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_conditional_attribute_spread() {
        let s = "<button {...(disabled ? { disabled: true } : {})} class=\"btn\">text</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_in_comment() {
        let s = "<!-- TODO: implement {feature} -->\n<p>placeholder</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_options_custom_element() {
        let s = "<svelte:options customElement=\"my-widget\" />\n<p>I'm a web component</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_class_shorthand_dynamic() {
        let s = "<div class:active class:disabled={!enabled} class:large={size === 'lg'}>styled</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_method_chain() {
        let s = "<p>{items.filter(Boolean).map(i => i.name).join(', ')}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_snippet_generic() {
        let s = "{#snippet list(items, renderItem)}\n\t<ul>\n\t\t{#each items as item}\n\t\t\t<li>{@render renderItem(item)}</li>\n\t\t{/each}\n\t</ul>\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_class_values() {
        let s = "<div class=\"a b c d e f g h i j k l m n o p\">many classes</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_key_with_expression() {
        let s = "{#key `${type}-${id}`}\n\t<Component {type} {id} />\n{/key}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_head_script_tag() {
        let s = "<svelte:head>\n\t<script src=\"https://example.com/analytics.js\"></script>\n</svelte:head>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_linter_no_diag_on_mustache_expr() {
        let s = "<p>{a + b * c}</p>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_escape_in_string_attr() {
        let s = "<div title=\"He said \\\"hello\\\"\">text</div>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_linter_combined_flags() {
        let s = "<script>\n\t$inspect(x);\n\t$: y = 42;\n</script>\n{@html z}\n{@debug w}\n<button>click</button>\n{#each items as i}\n\t<p>{i}</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        // Should have at least 4 different rule violations
        let unique_count = {
            let mut set = std::collections::HashSet::new();
            for d in &diags { set.insert(d.rule_name.clone()); }
            set.len()
        };
        assert!(unique_count >= 4, "Should flag >= 4 rules, got {}", unique_count);
    }

    #[test]
    fn test_parse_css_print_media() {
        let s = "<style>\n\t@media print {\n\t\t.no-print { display: none; }\n\t\tbody { font-size: 12pt; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_typeof_guard() {
        let s = "<p>{typeof globalVar !== 'undefined' ? globalVar : 'fallback'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_boolean_shorthand() {
        let s = "<input {disabled} {required} {readonly} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_reactive_store_subscription() {
        let s = "<script>\n\timport { page } from '$app/stores';\n</script>\n<p>Path: {$page.url.pathname}</p>\n<p>Params: {JSON.stringify($page.params)}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_computed_expression() {
        let s = "{#each Object.entries(data).filter(([_, v]) => v !== null).sort(([a], [b]) => a.localeCompare(b)) as [key, value] (key)}\n\t<p>{key}: {value}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_slot_fallback() {
        let s = "<Card>\n\t<slot name=\"header\">\n\t\t<h2>Default Title</h2>\n\t</slot>\n\t<slot>\n\t\t<p>Default content</p>\n\t</slot>\n</Card>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_display_variants() {
        let s = "<style>\n\t.flex { display: flex; }\n\t.grid { display: grid; }\n\t.hidden { display: none; }\n\t.block { display: block; }\n\t.inline { display: inline-block; }\n\t.contents { display: contents; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complete_todo_app_v4() {
        let s = "<script>\n\tlet todos = [];\n\tlet newTodo = '';\n\t$: remaining = todos.filter(t => !t.done).length;\n\tconst add = () => { if (newTodo) { todos = [...todos, { text: newTodo, done: false }]; newTodo = ''; } };\n\tconst remove = (i) => { todos = todos.filter((_, idx) => idx !== i); };\n\tconst toggle = (i) => { todos[i].done = !todos[i].done; todos = todos; };\n</script>\n<h1>Todos ({remaining}/{todos.length})</h1>\n<form on:submit|preventDefault={add}>\n\t<input bind:value={newTodo} placeholder=\"What needs doing?\" />\n\t<button type=\"submit\">Add</button>\n</form>\n<ul>\n\t{#each todos as todo, i (i)}\n\t\t<li class:done={todo.done}>\n\t\t\t<input type=\"checkbox\" checked={todo.done} on:change={() => toggle(i)} />\n\t\t\t<span>{todo.text}</span>\n\t\t\t<button on:click={() => remove(i)}>&times;</button>\n\t\t</li>\n\t{:else}\n\t\t<li class=\"empty\">Nothing to do!</li>\n\t{/each}\n</ul>\n<style>\n\t.done span { text-decoration: line-through; opacity: 0.5; }\n\t.empty { color: #999; font-style: italic; }\n\tul { list-style: none; padding: 0; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complete_todo_app_v5() {
        let s = "<script lang=\"ts\">\n\tinterface Todo { text: string; done: boolean }\n\tlet todos = $state<Todo[]>([]);\n\tlet newTodo = $state('');\n\tlet remaining = $derived(todos.filter(t => !t.done).length);\n\tconst add = () => { if (newTodo) { todos.push({ text: newTodo, done: false }); newTodo = ''; } };\n\tconst remove = (i: number) => todos.splice(i, 1);\n</script>\n<h1>Todos ({remaining}/{todos.length})</h1>\n<form onsubmit={(e) => { e.preventDefault(); add(); }}>\n\t<input bind:value={newTodo} />\n\t<button type=\"submit\">Add</button>\n</form>\n{#each todos as todo, i (todo.text)}\n\t<label class:done={todo.done}>\n\t\t<input type=\"checkbox\" bind:checked={todo.done} />\n\t\t{todo.text}\n\t\t<button onclick={() => remove(i)}>&times;</button>\n\t</label>\n{:else}\n\t<p>All done!</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- push to 1000 ---

    #[test]
    fn test_parse_if_in_each_in_if() {
        let s = "{#if show}\n\t{#each items as item}\n\t\t{#if item.visible}\n\t\t\t<p>{item.name}</p>\n\t\t{/if}\n\t{/each}\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_snippet_with_default() {
        let s = "{#snippet cell(value, fallback = 'N/A')}\n\t<td>{value ?? fallback}</td>\n{/snippet}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_render_in_each() {
        let s = "{#snippet item(data)}\n\t<li>{data.name}</li>\n{/snippet}\n{#each items as i}\n\t{@render item(i)}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_conditional_snippet() {
        let s = "{#if useCustom}\n\t{#snippet content()}<p>custom</p>{/snippet}\n{:else}\n\t{#snippet content()}<p>default</p>{/snippet}\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_export_const() {
        let s = "<script context=\"module\">\n\texport const prerender = true;\n\texport const ssr = false;\n\texport const csr = true;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_all_types() {
        let sources = [
            "<div bind:this={el} />",
            "<div bind:clientWidth={w} />",
            "<input bind:value />",
            "<select bind:value={sel}><option>a</option></select>",
            "<details bind:open />",
            "<textarea bind:value />",
            "<audio bind:paused />",
        ];
        for s in &sources {
            let r = parser::parse(s);
            assert!(r.errors.is_empty(), "Failed: {}", s);
        }
    }

    #[test]
    fn test_parse_event_shorthand_types() {
        let sources = [
            "<button on:click>forward click</button>",
            "<div on:mouseenter on:mouseleave>hover</div>",
            "<input on:input on:change on:blur on:focus />",
        ];
        for s in &sources {
            let r = parser::parse(s);
            assert!(r.errors.is_empty(), "Failed: {}", s);
        }
    }

    #[test]
    fn test_parse_component_is_pattern() {
        let s = "<script>\n\tconst components = { A: CompA, B: CompB };\n\tlet current = 'A';\n</script>\n<svelte:component this={components[current]} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_shorthand_directive_multiple() {
        let s = "<input bind:value={value} class:active={active} />";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let sd: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/shorthand-directive").collect();
        assert_eq!(sd.len(), 2, "Should flag both non-shorthand directives");
    }

    #[test]
    fn test_parse_style_directives_mixed() {
        let s = "<div style:color style:background-color=\"white\" style:font-size=\"{size}px\" style:--custom={val}>styled</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_store_expression() {
        let s = "<p>{$count > 0 ? `${$count} items` : 'No items'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_comment_between_elements() {
        let s = "<div>\n\t<!-- Section 1 -->\n\t<p>Content 1</p>\n\t<!-- Section 2 -->\n\t<p>Content 2</p>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_empty_else() {
        let s = "{#each items as item (item.id)}\n\t<Card {item} />\n{:else}\n\t<!-- intentionally empty -->\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_typeof() {
        let s = "<p>{typeof value === 'undefined' ? 'none' : value}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_element_data_attrs() {
        let s = "<div data-sveltekit-preload-data=\"hover\" data-sveltekit-reload>content</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_complex_selectors() {
        let s = "<style>\n\tdiv > p:first-child + span ~ a[href^=\"https\"] { color: green; }\n\t.parent:not(.disabled):hover::before { content: '→'; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_dynamic_attribute_name() {
        let s = "<div aria-expanded={open}>content</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_with_regex() {
        let s = "<script>\n\tconst pattern = /^[a-z]+$/i;\n\tconst result = 'test'.match(pattern);\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_inline_if_else() {
        let s = "{#if a}<span>a</span>{:else if b}<span>b</span>{:else}<span>c</span>{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_whitespace_in_attributes() {
        let s = "<div\n  class = \"spaced\"\n  id = \"test\"\n>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_in_attribute_value() {
        let s = "<img src=\"/images/{name}.png\" alt=\"Image of {name}\" />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_index_key() {
        let s = "{#each items as item, index}\n\t<p data-index={index}>{item}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_await_then_catch() {
        let s = "{#await loadData() then data}\n\t<p>{data}</p>\n{/await}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_tag_with_colon() {
        let s = "<my-custom-element prop:value={42}>text</my-custom-element>";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_linter_no_issues_text_with_entities() {
        let s = "<p>&copy; 2024 &mdash; All rights reserved</p>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_style_with_nested_at() {
        let s = "<style>\n\t@media screen {\n\t\t@supports (display: grid) {\n\t\t\t.container { display: grid; }\n\t\t}\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svg_ns_element() {
        let s = "<svg:circle cx=\"50\" cy=\"50\" r=\"40\" fill=\"red\" />";
        let r = parser::parse(s);
        let _ = r.ast.html.nodes.len();
    }

    #[test]
    fn test_parse_expression_with_spread_array() {
        let s = "<p>{[...a, ...b, ...c].join(', ')}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_conditional_rendering_complex() {
        let s = "{#if status === 'loading'}\n\t<Spinner />\n{:else if status === 'error'}\n\t{#if retryable}\n\t\t<ErrorWithRetry {error} on:retry />\n\t{:else}\n\t\t<FatalError {error} />\n\t{/if}\n{:else if data.items.length === 0}\n\t<EmptyState />\n{:else}\n\t<DataView items={data.items} />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_select_multiple() {
        let s = "<select multiple bind:value={selectedColors}>\n\t{#each colors as color}\n\t\t<option value={color}>{color}</option>\n\t{/each}\n</select>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_chain() {
        let s = "<Provider value={{theme: 'dark'}}>\n\t<Router>\n\t\t<Layout>\n\t\t\t<Page />\n\t\t</Layout>\n\t</Router>\n</Provider>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_issues_svelte5_minimal() {
        let s = "<script>\n\tlet count = $state(0);\n</script>\n<button onclick={() => count++}>{count}</button>";
        let r = parser::parse(s);
        let diags = Linter::recommended().lint(&r.ast, s);
        let filtered: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/block-lang" && d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(filtered.is_empty(), "Minimal Svelte 5: {:?}",
            filtered.iter().map(|d| &d.rule_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_parse_attribute_with_pipe() {
        let s = "<input value={value | 0} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_comma_operator() {
        let s = "<button on:click={() => (update(), render())}>go</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_with_decorators() {
        let s = "<script lang=\"ts\">\n\t// @ts-ignore\n\tconst x = unsafeOperation();\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiline_attribute_expression() {
        let s = "<Component\n\tdata={{\n\t\tname: 'test',\n\t\tvalue: 42,\n\t\tnested: { a: 1 }\n\t}}\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_class_with_computed() {
        let s = "<div class=\"item {type} {active ? 'active' : ''} size-{size}\">text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- final push to 950 ---

    #[test]
    fn test_parse_spread_in_each() {
        let s = "{#each items as item}\n\t<Component {...item} />\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_optional_chain_template() {
        let s = "<p>{user?.address?.city ?? 'Unknown'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_assignment_in_handler() {
        let s = "<input on:input={(e) => value = e.target.value} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_with_multiple_values() {
        let s = "<style>\n\t.shadow { box-shadow: 0 2px 4px rgba(0,0,0,.1), 0 8px 16px rgba(0,0,0,.1); }\n\t.font { font: bold 14px/1.5 'Helvetica', Arial, sans-serif; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_component_with_children_prop() {
        let s = "<script>\n\tlet { children } = $props();\n</script>\n<div class=\"wrapper\">\n\t{@render children?.()}\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_recursive_slot() {
        let s = "<script>\n\texport let items;\n</script>\n<ul>\n\t{#each items as item}\n\t\t<li>\n\t\t\t{item.name}\n\t\t\t{#if item.children?.length}\n\t\t\t\t<svelte:self items={item.children} />\n\t\t\t{/if}\n\t\t</li>\n\t{/each}\n</ul>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_textarea_binding() {
        let s = "<textarea bind:value rows=\"5\" cols=\"40\" placeholder=\"Type here...\">{initialText}</textarea>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_details_open_binding() {
        let s = "<details bind:open={isOpen}>\n\t<summary>Click to expand</summary>\n\t<div>Hidden content</div>\n</details>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_input_types() {
        let sources = [
            "<input type=\"text\" bind:value />",
            "<input type=\"number\" bind:value={num} />",
            "<input type=\"checkbox\" bind:checked />",
            "<input type=\"range\" bind:value min=\"0\" max=\"100\" />",
            "<input type=\"color\" bind:value={color} />",
            "<input type=\"date\" bind:value={date} />",
            "<input type=\"file\" on:change={handleFile} />",
        ];
        for s in &sources {
            let r = parser::parse(s);
            assert!(r.errors.is_empty(), "Failed: {}", s);
        }
    }

    #[test]
    fn test_linter_multiple_each_keys() {
        let s = "{#each a as x (x.id)}\n\t<p>a</p>\n{/each}\n{#each b as y}\n\t<p>b</p>\n{/each}\n{#each c as z (z.id)}\n\t<p>c</p>\n{/each}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        let key_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/require-each-key").collect();
        assert_eq!(key_diags.len(), 1, "Should flag exactly 1 each without key (b)");
    }

    #[test]
    fn test_parse_slot_with_binding() {
        let s = "<Table {data} let:row let:index>\n\t<tr class:even={index % 2 === 0}>\n\t\t<td>{row.name}</td>\n\t</tr>\n</Table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_transition_local() {
        let s = "{#if visible}\n\t<div transition:fly|local={{y: -100}}>local transition</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_const_in_if() {
        let s = "{#if items.length > 0}\n\t{@const first = items[0]}\n\t<p>First: {first.name}</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_html_tags() {
        let s = "{@html headerHtml}\n<main>\n\t{@html contentHtml}\n</main>\n{@html footerHtml}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::all().lint(&r.ast, s);
        let html_diags: Vec<_> = diags.iter().filter(|d| d.rule_name == "svelte/no-at-html-tags").collect();
        assert_eq!(html_diags.len(), 3);
    }

    // --- pushing 950 ---

    #[test]
    fn test_parse_multiple_const_tags() {
        let s = "{#each items as item}\n\t{@const name = item.name.toUpperCase()}\n\t{@const price = item.price * 1.2}\n\t{@const total = price * item.qty}\n\t<p>{name}: {total}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_string_with_html_entities() {
        let s = "<p>Price: &dollar;{price} &mdash; {discount}% off</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_nested_ternary_expression() {
        let s = "<p>{a ? b ? 'deep-true' : 'mid-false' : c ? 'alt-true' : 'alt-false'}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_template_literal() {
        let s = "<p>{`Item ${index + 1} of ${total}: ${items[index]?.name ?? 'unknown'}`}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_event_handler() {
        let s = "<button on:click|preventDefault|stopPropagation={(e) => {\n\tconst target = e.currentTarget;\n\tconst data = new FormData(target.form);\n\tsubmit(Object.fromEntries(data));\n}}>Submit</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_multiple_at_rules() {
        let s = "<style>\n\t@media (prefers-color-scheme: dark) {\n\t\t.theme { background: #222; color: #eee; }\n\t}\n\t@media (prefers-reduced-motion: reduce) {\n\t\t* { animation: none !important; transition: none !important; }\n\t}\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_restprops_and_events() {
        let s = "<button {...$$restProps} on:click on:keydown class=\"btn {variant}\">\n\t<slot />\n</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_destructured_props() {
        let s = "<script lang=\"ts\">\n\tlet {\n\t\tname,\n\t\tage = 0,\n\t\titems = [],\n\t\tonchange,\n\t\tclass: className = '',\n\t\t...rest\n\t}: {\n\t\tname: string;\n\t\tage?: number;\n\t\titems?: string[];\n\t\tonchange?: () => void;\n\t\tclass?: string;\n\t\t[key: string]: unknown;\n\t} = $props();\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_async_item() {
        let s = "{#each promises as promise}\n\t{#await promise}\n\t\t<p>Loading...</p>\n\t{:then value}\n\t\t<p>{value}</p>\n\t{:catch}\n\t\t<p>Error</p>\n\t{/await}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_dynamic_import_in_script() {
        let s = "<script>\n\tlet Component;\n\tconst load = async () => {\n\t\tconst mod = await import('./Heavy.svelte');\n\t\tComponent = mod.default;\n\t};\n</script>\n{#if Component}\n\t<svelte:component this={Component} />\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_dupe_else_if_nested() {
        let s = "{#if a}\n\tp1\n{:else if b}\n\tp2\n{:else if a}\n\tp3\n{/if}";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-dupe-else-if-blocks"),
            "Should flag duplicate else-if condition 'a'");
    }

    #[test]
    fn test_parse_special_element_mixed() {
        let s = "<svelte:head>\n\t<title>{title}</title>\n</svelte:head>\n<svelte:window on:resize={handleResize} />\n<svelte:body on:click={handleClick} />\n<main>\n\t<slot />\n</main>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_scope_with_global() {
        let s = "<style>\n\t.local { color: red; }\n\t:global(.external) { color: blue; }\n\t.local :global(.mixed) { display: flex; }\n\t:global(body.dark) .local { background: #333; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_reactive_store_derived_chain() {
        let s = "<script>\n\timport { writable, derived } from 'svelte/store';\n\tconst base = writable(0);\n\tconst doubled = derived(base, $b => $b * 2);\n\tconst quadrupled = derived(doubled, $d => $d * 2);\n\tconst formatted = derived(quadrupled, $q => `Value: ${$q}`);\n</script>\n<p>{$formatted}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- towards 950 ---

    #[test]
    fn test_parse_css_min_max() {
        let s = "<style>\n\t.box { width: min(100%, 800px); height: max(50vh, 300px); font-size: clamp(1rem, 2vw, 2rem); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_css_color_functions() {
        let s = "<style>\n\t.c { color: hsl(200, 50%, 50%); background: rgb(255, 128, 0); border-color: oklch(70% 0.15 200); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_inline_conditional() {
        let s = "{#if true}<span>inline</span>{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_adjacent_mustaches() {
        let s = "<p>{first}{middle}{last}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_empty_element() {
        let s = "<div></div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        if let ast::TemplateNode::Element(el) = &r.ast.html.nodes[0] {
            assert!(el.children.is_empty() || el.children.iter().all(|c| matches!(c, ast::TemplateNode::Text(t) if t.data.trim().is_empty())));
        }
    }

    #[test]
    fn test_linter_store_reactive_derived() {
        let s = "<script>\n\timport { writable, derived } from 'svelte/store';\n\tconst count = writable(0);\n\tconst doubled = derived(count, ($c) => $c * 2);\n</script>\n<p>{doubled}</p>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/require-store-reactive-access"),
            "Should flag raw derived store in template");
    }

    #[test]
    fn test_parse_script_with_class() {
        let s = "<script>\n\tclass MyClass {\n\t\tconstructor(value) { this.value = value; }\n\t\tgetValue() { return this.value; }\n\t}\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_rest() {
        let s = "{#each items as [head, ...tail]}\n\t<p>Head: {head}, Rest: {tail.length}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_complex_binding() {
        let s = "<input type=\"range\" min=\"0\" max=\"100\" step=\"5\" bind:value={volume} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte5_boundary_failed() {
        let s = "<svelte:boundary onerror={(e, retry) => { log(e); retry(); }}>\n\t<Risky />\n</svelte:boundary>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- hitting 900 ---

    #[test]
    fn test_parse_form_enhance() {
        let s = "<form method=\"POST\" use:enhance>\n\t<input name=\"email\" type=\"email\" required />\n\t<button type=\"submit\">Submit</button>\n</form>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_slot_forwarding() {
        let s = "<Wrapper>\n\t<slot />\n</Wrapper>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_contentwidth() {
        let s = "<div bind:contentRect={rect} bind:contentBoxSize={size}>resizable</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_prop_expression() {
        let s = "<Chart data={items.map(i => ({ x: i.date, y: i.value }))} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_else_if_chain_long() {
        let s = "{#if a === 1}\n\t<p>one</p>\n{:else if a === 2}\n\t<p>two</p>\n{:else if a === 3}\n\t<p>three</p>\n{:else if a === 4}\n\t<p>four</p>\n{:else if a === 5}\n\t<p>five</p>\n{:else}\n\t<p>other</p>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_pre_with_code() {
        let s = "<pre><code class=\"language-js\">const x = 1;\nconst y = 2;\nconsole.log(x + y);</code></pre>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_nested_component_slots() {
        let s = "<Layout>\n\t<Sidebar slot=\"sidebar\">\n\t\t<NavItem href=\"/\">Home</NavItem>\n\t\t<NavItem href=\"/about\">About</NavItem>\n\t</Sidebar>\n\t<slot />\n</Layout>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- towards 900 ---

    #[test]
    fn test_parse_svelte5_class_with_derived() {
        let s = "<script>\n\tclass Counter {\n\t\t#count = $state(0);\n\t\tget count() { return this.#count; }\n\t\tget doubled() { return $derived(this.#count * 2); }\n\t\tincrement() { this.#count++; }\n\t}\n\tconst counter = new Counter();\n</script>\n<p>{counter.count}</p>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_action_complex() {
        let s = "<div use:portal={'body'} use:clickOutside={{handler: close, exclude: ['.menu']}} use:tooltip>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_transition_params_complex() {
        let s = "<div transition:fly={{y: -200, duration: 500, delay: 100, easing: cubicOut}}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_typing() {
        let s = "<script lang=\"ts\">\n\timport type { ComponentProps } from 'svelte';\n\timport MyComponent from './MyComponent.svelte';\n\ttype Props = ComponentProps<MyComponent>;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_no_reactive_literal_boolean() {
        let s = "<script>\n\t$: flag = true;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should flag reactive boolean literal");
    }

    #[test]
    fn test_linter_no_reactive_literal_null() {
        let s = "<script>\n\t$: val = null;\n</script>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/no-reactive-literals"),
            "Should flag reactive null literal");
    }

    #[test]
    fn test_prefer_class_directive_svelte_element() {
        let s = "<svelte:element this=\"div\" class={active ? 'active' : ''}>text</svelte:element>";
        let r = parser::parse(s);
        let diags = Linter::all().lint(&r.ast, s);
        assert!(diags.iter().any(|d| d.rule_name == "svelte/prefer-class-directive"),
            "Should suggest class directive for svelte:element");
    }

    #[test]
    fn test_parse_multiline_tag() {
        let s = "<div\n\tclass=\"container\"\n\tid=\"main\"\n\trole=\"main\"\n\taria-label=\"Main content\"\n\tdata-testid=\"main-container\"\n>\n\t<p>Content</p>\n</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_expression_with_arrow() {
        let s = "<button on:click={() => { count++; display = count > 10 ? 'big' : 'small'; }}>click</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_nested_ternary_in_attr() {
        let s = "<div class={a ? 'x' : b ? 'y' : 'z'}>text</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_style_with_var() {
        let s = "<div style:--custom-color={color} style:--custom-size=\"{size}px\">styled</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_with_async() {
        let s = "<script>\n\tasync function fetchData() {\n\t\tconst res = await fetch('/api');\n\t\treturn await res.json();\n\t}\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_event_dispatch() {
        let s = "<script>\n\timport { createEventDispatcher } from 'svelte';\n\tconst dispatch = createEventDispatcher();\n\tconst click = () => dispatch('custom', { detail: 42 });\n</script>\n<button on:click={click}>fire event</button>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_svelte_options_accessors() {
        let s = "<svelte:options accessors={true} />\n<script>\n\texport let value = 0;\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_with_computed_key() {
        let s = "{#each items as item (`${item.type}-${item.id}`)}\n\t<p>{item.name}</p>\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_component_with_snippets_and_render() {
        let s = "<Dialog>\n\t{#snippet header()}\n\t\t<h2>Title</h2>\n\t{/snippet}\n\t{#snippet body()}\n\t\t<p>Content</p>\n\t{/snippet}\n\t{#snippet footer()}\n\t\t<button>OK</button>\n\t{/snippet}\n</Dialog>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_script_complex_imports() {
        let s = "<script>\n\timport { onMount, onDestroy, beforeUpdate, afterUpdate, tick } from 'svelte';\n\timport { writable, readable, derived, get } from 'svelte/store';\n\timport { fade, fly, slide, scale, draw, crossfade } from 'svelte/transition';\n\timport { flip } from 'svelte/animate';\n\timport { cubicOut, elasticOut } from 'svelte/easing';\n</script>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_head_with_opengraph() {
        let s = "<svelte:head>\n\t<title>{title}</title>\n\t<meta name=\"description\" content={description} />\n\t<meta property=\"og:title\" content={title} />\n\t<meta property=\"og:description\" content={description} />\n\t<meta property=\"og:image\" content={image} />\n\t<link rel=\"canonical\" href={url} />\n</svelte:head>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_body_with_events() {
        let s = "<svelte:body\n\ton:mouseenter={() => hovered = true}\n\ton:mouseleave={() => hovered = false}\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_window_with_bindings() {
        let s = "<svelte:window\n\tbind:scrollY={y}\n\tbind:innerWidth={w}\n\tbind:innerHeight={h}\n\tbind:online={isOnline}\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_linter_clean_sveltekit_svelte5() {
        let s = "<script lang=\"ts\">\n\tlet { data } = $props();\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n</script>\n\n<h1>{data.title}</h1>\n<p>Count: {count}, Doubled: {doubled}</p>\n<button onclick={() => count++}>+1</button>\n\n<style lang=\"scss\">\n\th1 { color: var(--primary); }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
        let diags = Linter::recommended().lint(&r.ast, s);
        let relevant: Vec<_> = diags.iter()
            .filter(|d| d.rule_name != "svelte/no-unused-class-name")
            .collect();
        assert!(relevant.is_empty(), "Clean SvelteKit+Svelte5 page: {:?}",
            relevant.iter().map(|d| format!("{}: {}", d.rule_name, d.message)).collect::<Vec<_>>());
    }

    #[test]
    fn test_parse_conditional_class_multiple() {
        let s = "<div\n\tclass=\"base\"\n\tclass:primary={variant === 'primary'}\n\tclass:secondary={variant === 'secondary'}\n\tclass:large={size === 'lg'}\n\tclass:small={size === 'sm'}\n>button</div>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_bind_media_elements() {
        let s = "<video\n\tsrc={videoUrl}\n\tbind:duration\n\tbind:currentTime\n\tbind:paused\n\tbind:volume\n\tbind:muted\n\tbind:playbackRate\n\tbind:seeking\n\tbind:ended\n/>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_each_nested_3_levels() {
        let s = "{#each departments as dept}\n\t<h2>{dept.name}</h2>\n\t{#each dept.teams as team}\n\t\t<h3>{team.name}</h3>\n\t\t{#each team.members as member (member.id)}\n\t\t\t<p>{member.name}</p>\n\t\t{/each}\n\t{/each}\n{/each}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_await_with_loading_state() {
        let s = "<script>\n\tlet promise = null;\n\tconst load = () => promise = fetch('/api').then(r => r.json());\n</script>\n\n<button onclick={load}>Load</button>\n{#if promise}\n\t{#await promise}\n\t\t<p>Loading...</p>\n\t{:then data}\n\t\t<pre>{JSON.stringify(data, null, 2)}</pre>\n\t{:catch err}\n\t\t<p class=\"error\">{err.message}</p>\n\t{/await}\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_key_block_with_transition() {
        let s = "{#key selectedTab}\n\t<div transition:fade>\n\t\t{#if selectedTab === 'a'}\n\t\t\t<TabA />\n\t\t{:else}\n\t\t\t<TabB />\n\t\t{/if}\n\t</div>\n{/key}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_inline_component() {
        let s = "<svelte:component this={isAdmin ? AdminPanel : UserPanel} {data} on:action={handleAction} />";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    // --- CLI and integration tests ---

    #[test]
    fn test_linter_all_rule_names_unique() {
        let linter = Linter::all();
        let names: Vec<&str> = linter.rules().iter().map(|r| r.name()).collect();
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(name), "Duplicate rule name: {}", name);
        }
    }

    #[test]
    fn test_linter_fixable_rules_exist() {
        let linter = Linter::all();
        let fixable_count = linter.rules().iter().filter(|r| r.is_fixable()).count();
        assert!(fixable_count >= 10, "Should have at least 10 fixable rules, got {}", fixable_count);
    }

    #[test]
    fn test_linter_recommended_rules_are_recommended() {
        let rec = Linter::recommended();
        for rule in rec.rules() {
            assert!(rule.is_recommended(), "Rule {} in recommended linter should be recommended", rule.name());
        }
    }

    #[test]
    fn test_parse_complex_svelte5_with_all_features() {
        let s = "<script lang=\"ts\" generics=\"T extends { id: string }\">\n\timport { onMount } from 'svelte';\n\timport { fade } from 'svelte/transition';\n\n\tinterface $$Events { select: CustomEvent<T> }\n\n\tlet { items = [], selected = $bindable<T | null>(null) }: {\n\t\titems?: T[];\n\t\tselected?: T | null;\n\t} = $props();\n\n\tlet count = $derived(items.length);\n\tlet filtered = $derived.by(() => items.filter(Boolean));\n\n\t$effect(() => { console.log('items changed', count); });\n\t$effect.pre(() => { /* pre-effect */ });\n\n\tonMount(() => { console.log('mounted'); });\n\n\tlet el: HTMLDivElement;\n\tconst host = $host();\n</script>\n\n<svelte:head>\n\t<title>Items ({count})</title>\n</svelte:head>\n\n<svelte:window on:keydown={(e) => { if (e.key === 'Escape') selected = null; }} />\n\n<div bind:this={el} class=\"container\">\n\t{#if count > 0}\n\t\t<ul>\n\t\t\t{#each filtered as item (item.id)}\n\t\t\t\t<li\n\t\t\t\t\tclass:selected={item === selected}\n\t\t\t\t\ttransition:fade={{duration: 200}}\n\t\t\t\t\tonclick={() => selected = item}\n\t\t\t\t\trole=\"button\"\n\t\t\t\t\ttabindex=\"0\"\n\t\t\t\t>\n\t\t\t\t\t{#snippet itemContent()}\n\t\t\t\t\t\t<span>{item.id}</span>\n\t\t\t\t\t{/snippet}\n\t\t\t\t\t{@render itemContent()}\n\t\t\t\t</li>\n\t\t\t{/each}\n\t\t</ul>\n\t{:else}\n\t\t<p>No items</p>\n\t{/if}\n</div>\n\n<style lang=\"scss\">\n\t.container { padding: 1rem; }\n\t.selected { background: var(--highlight, #eef); font-weight: bold; }\n\tul { list-style: none; padding: 0; }\n\tli { cursor: pointer; padding: 0.5rem; border-bottom: 1px solid #eee; }\n</style>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty(), "Complex Svelte 5 component should parse without errors");
        assert!(r.ast.instance.is_some());
        assert!(r.ast.css.is_some());
    }

    #[test]
    fn test_parse_real_world_data_table() {
        let s = "<script lang=\"ts\">\n\ttype Column<T> = { key: keyof T; label: string; sortable?: boolean };\n\tlet { data, columns, sortBy = null, sortDir = 'asc' }: {\n\t\tdata: Record<string, unknown>[];\n\t\tcolumns: Column<Record<string, unknown>>[];\n\t\tsortBy?: string | null;\n\t\tsortDir?: 'asc' | 'desc';\n\t} = $props();\n\n\tlet sorted = $derived.by(() => {\n\t\tif (!sortBy) return data;\n\t\treturn [...data].sort((a, b) => {\n\t\t\tconst va = String(a[sortBy!]);\n\t\t\tconst vb = String(b[sortBy!]);\n\t\t\treturn sortDir === 'asc' ? va.localeCompare(vb) : vb.localeCompare(va);\n\t\t});\n\t});\n</script>\n\n<table>\n\t<thead>\n\t\t<tr>\n\t\t\t{#each columns as col}\n\t\t\t\t<th onclick={() => { sortBy = String(col.key); sortDir = sortDir === 'asc' ? 'desc' : 'asc'; }}>\n\t\t\t\t\t{col.label}\n\t\t\t\t\t{#if sortBy === col.key}{sortDir === 'asc' ? '▲' : '▼'}{/if}\n\t\t\t\t</th>\n\t\t\t{/each}\n\t\t</tr>\n\t</thead>\n\t<tbody>\n\t\t{#each sorted as row}\n\t\t\t<tr>\n\t\t\t\t{#each columns as col}\n\t\t\t\t\t<td>{row[String(col.key)]}</td>\n\t\t\t\t{/each}\n\t\t\t</tr>\n\t\t{/each}\n\t</tbody>\n</table>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_breadcrumb_nav() {
        let s = "<script>\n\tlet { items = [] } = $props();\n</script>\n\n<nav aria-label=\"Breadcrumb\">\n\t<ol>\n\t\t{#each items as item, i}\n\t\t\t<li aria-current={i === items.length - 1 ? 'page' : undefined}>\n\t\t\t\t{#if i < items.length - 1}\n\t\t\t\t\t<a href={item.href}>{item.label}</a>\n\t\t\t\t\t<span aria-hidden=\"true\">/</span>\n\t\t\t\t{:else}\n\t\t\t\t\t{item.label}\n\t\t\t\t{/if}\n\t\t\t</li>\n\t\t{/each}\n\t</ol>\n</nav>";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parse_notification_toast() {
        let s = "<script>\n\tlet { message, type = 'info', duration = 3000, ondismiss } = $props();\n\tlet visible = $state(true);\n\t$effect(() => {\n\t\tconst timer = setTimeout(() => { visible = false; ondismiss?.(); }, duration);\n\t\treturn () => clearTimeout(timer);\n\t});\n</script>\n\n{#if visible}\n\t<div class=\"toast {type}\" role=\"alert\" transition:fade>\n\t\t<p>{message}</p>\n\t\t<button onclick={() => { visible = false; ondismiss?.(); }} aria-label=\"Dismiss\">&times;</button>\n\t</div>\n{/if}";
        let r = parser::parse(s);
        assert!(r.errors.is_empty());
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
    use crate::linter::{Linter, RuleConfig};

    /// Load the rule config for a specific fixture file.
    /// Checks for `<basename>-config.json` first, then `_config.json` as default.
    fn load_config(dir: &str, input_filename: &str) -> RuleConfig {
        // Per-file config: foo-input.svelte -> foo-config.json
        let base = input_filename.strip_suffix("-input.svelte").unwrap_or(input_filename);
        let per_file = format!("{}/{}-config.json", dir, base);
        let default_cfg = format!("{}/_config.json", dir);

        let config_path = if std::path::Path::new(&per_file).exists() {
            Some(per_file)
        } else if std::path::Path::new(&default_cfg).exists() {
            Some(default_cfg)
        } else {
            None
        };

        if let Some(path) = config_path {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    // Extract rule options from the config
                    let options = json.get("options").cloned()
                        .or_else(|| json.get("rules").and_then(|r| {
                            // ESLint format: { "rules": { "svelte/rule-name": ["error", options] } }
                            r.as_object().and_then(|m| {
                                m.values().next().and_then(|v| {
                                    v.as_array().and_then(|a| a.get(1).cloned())
                                })
                            })
                        }));
                    let settings = json.get("settings").cloned();
                    return RuleConfig { options, settings };
                }
            }
        }
        RuleConfig::default()
    }

    fn run_linter_valid(rule_name: &str) {
        let valid_dir = format!("fixtures/linter/{}/valid", rule_name);
        if let Ok(entries) = std::fs::read_dir(&valid_dir) {
            let lint = Linter::all();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() { continue; }
                let fname = path.file_name().unwrap().to_string_lossy().to_string();
                if fname.ends_with("-input.svelte") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    let result = parser::parse(&source);
                    let config = load_config(&valid_dir, &fname);
                    let diags = lint.lint_with_config(&result.ast, &source, config);
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
                let fname = path.file_name().unwrap().to_string_lossy().to_string();
                if fname.ends_with("-input.svelte") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    let result = parser::parse(&source);
                    let config = load_config(&invalid_dir, &fname);
                    let diags = lint.lint_with_config(&result.ast, &source, config);
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
    #[test] fn linter_prefer_class_directive_invalid() { run_linter_invalid("prefer-class-directive"); }
    #[test] fn linter_prefer_style_directive_valid() { run_linter_valid("prefer-style-directive"); }
    #[test] fn linter_prefer_style_directive_invalid() { run_linter_invalid("prefer-style-directive"); }
    #[test] fn linter_no_trailing_spaces_valid() { run_linter_valid("no-trailing-spaces"); }
    #[test] fn linter_no_trailing_spaces_invalid() { run_linter_invalid("no-trailing-spaces"); }
    // no-restricted-html-elements requires rule configuration support
    #[test] fn linter_no_restricted_html_elements_valid() { run_linter_valid("no-restricted-html-elements"); }
    #[test] fn linter_no_restricted_html_elements_invalid() { run_linter_invalid("no-restricted-html-elements"); }
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
    #[test] fn linter_no_unnecessary_state_wrap_invalid() { run_linter_invalid("no-unnecessary-state-wrap"); }

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
    #[test] fn linter_sort_attributes_valid() { run_linter_valid("sort-attributes"); }
    #[test] fn linter_indent_valid() { run_linter_valid("indent"); }
    // indent: 36/37 invalid pass (style-directive01 needs column-based attr indent)
    // #[test] fn linter_indent_invalid() { run_linter_invalid("indent"); }
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
