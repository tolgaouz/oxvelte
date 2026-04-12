# oxvelte Rule Reference

79 rules total. All use the same names and options as eslint-plugin-svelte.

## Recommended rules (enabled by default)

| Rule | Fixable |
|------|---------|
| `svelte/comment-directive` | |
| `svelte/infinite-reactive-loop` | |
| `svelte/no-at-debug-tags` | |
| `svelte/no-at-html-tags` | |
| `svelte/no-dom-manipulating` | |
| `svelte/no-dupe-else-if-blocks` | |
| `svelte/no-dupe-on-directives` | |
| `svelte/no-dupe-style-properties` | |
| `svelte/no-dupe-use-directives` | |
| `svelte/no-export-load-in-svelte-module-in-kit-pages` | |
| `svelte/no-immutable-reactive-statements` | |
| `svelte/no-inner-declarations` | |
| `svelte/no-inspect` | |
| `svelte/no-navigation-without-resolve` | |
| `svelte/no-not-function-handler` | |
| `svelte/no-object-in-text-mustaches` | |
| `svelte/no-raw-special-elements` | Yes |
| `svelte/no-reactive-functions` | |
| `svelte/no-reactive-literals` | |
| `svelte/no-reactive-reassign` | |
| `svelte/no-shorthand-style-property-overrides` | |
| `svelte/no-store-async` | |
| `svelte/no-svelte-internal` | |
| `svelte/no-unnecessary-state-wrap` | |
| `svelte/no-unknown-style-directive-property` | |
| `svelte/no-unused-props` | |
| `svelte/no-unused-svelte-ignore` | |
| `svelte/no-useless-children-snippet` | |
| `svelte/no-useless-mustaches` | Yes |
| `svelte/prefer-svelte-reactivity` | |
| `svelte/prefer-writable-derived` | |
| `svelte/require-each-key` | |
| `svelte/require-store-reactive-access` | Yes |
| `svelte/system` | |
| `svelte/valid-each-key` | |
| `svelte/valid-prop-names-in-kit-pages` | |

## Non-recommended rules (opt-in)

| Rule | Fixable |
|------|---------|
| `svelte/block-lang` | |
| `svelte/button-has-type` | |
| `svelte/consistent-selector-style` | |
| `svelte/derived-has-same-inputs-outputs` | |
| `svelte/experimental-require-slot-types` | |
| `svelte/experimental-require-strict-events` | |
| `svelte/first-attribute-linebreak` | Yes |
| `svelte/html-closing-bracket-new-line` | Yes |
| `svelte/html-closing-bracket-spacing` | Yes |
| `svelte/html-quotes` | Yes |
| `svelte/html-self-closing` | Yes |
| `svelte/indent` | Yes |
| `svelte/max-attributes-per-line` | Yes |
| `svelte/max-lines-per-block` | |
| `svelte/mustache-spacing` | Yes |
| `svelte/no-add-event-listener` | |
| `svelte/no-dynamic-slot-name` | |
| `svelte/no-extra-reactive-curlies` | |
| `svelte/no-goto-without-base` | |
| `svelte/no-ignored-unsubscribe` | |
| `svelte/no-inline-styles` | |
| `svelte/no-navigation-without-base` | |
| `svelte/no-restricted-html-elements` | |
| `svelte/no-spaces-around-equal-signs-in-attribute` | Yes |
| `svelte/no-target-blank` | |
| `svelte/no-top-level-browser-globals` | |
| `svelte/no-trailing-spaces` | Yes |
| `svelte/no-unused-class-name` | |
| `svelte/prefer-class-directive` | Yes |
| `svelte/prefer-const` | Yes |
| `svelte/prefer-destructured-store-props` | |
| `svelte/prefer-style-directive` | Yes |
| `svelte/require-event-dispatcher-types` | |
| `svelte/require-event-prefix` | |
| `svelte/require-optimized-style-attribute` | |
| `svelte/require-store-callbacks-use-set-param` | |
| `svelte/require-stores-init` | |
| `svelte/shorthand-attribute` | Yes |
| `svelte/shorthand-directive` | Yes |
| `svelte/sort-attributes` | Yes |
| `svelte/spaced-html-comment` | Yes |
| `svelte/valid-compile` | |
| `svelte/valid-style-parse` | |

## Intentionally excluded from eslint-plugin-svelte

These rules exist in eslint-plugin-svelte but are NOT fully implemented in oxvelte:

- **`valid-compile`** — oxvelte has a basic stub, but the full rule requires the Svelte compiler. Your build step already catches these errors.
- **`no-unused-svelte-ignore`** — requires the Svelte compiler to know which diagnostics were suppressed. Svelte warns about this at build time.

## ESLint stack replacement mapping

| ESLint package | Replacement |
|---------------|-------------|
| `@eslint/js` (core JS rules) | `oxlint` |
| `typescript-eslint` | `oxlint --tsconfig` |
| `eslint-plugin-svelte` | **`oxvelte`** |
| `eslint-plugin-import` | `oxlint` (built-in import plugin) |
