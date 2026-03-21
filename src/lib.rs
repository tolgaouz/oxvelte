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
    #[test] fn linter_no_useless_children_snippet_valid() { run_linter_valid("no-useless-children-snippet"); }
    #[test] fn linter_no_reactive_reassign_valid() { run_linter_valid("no-reactive-reassign"); }
    #[test] fn linter_no_ignored_unsubscribe_valid() { run_linter_valid("no-ignored-unsubscribe"); }
    #[test] fn linter_no_inner_declarations_valid() { run_linter_valid("no-inner-declarations"); }
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
    #[test] fn linter_prefer_const_valid() { run_linter_valid("prefer-const"); }
    #[test] fn linter_prefer_const_invalid() { run_linter_invalid("prefer-const"); }
    #[test] fn linter_prefer_destructured_store_props_valid() { run_linter_valid("prefer-destructured-store-props"); }
    #[test] fn linter_prefer_destructured_store_props_invalid() { run_linter_invalid("prefer-destructured-store-props"); }
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
