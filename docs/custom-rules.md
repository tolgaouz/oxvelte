# Custom rules

Write lint rules in JavaScript and load them into oxvelte. Good for project-specific conventions that don't belong in the shared ruleset — forbidden imports, naming schemes, required attributes, house-style template patterns.

Custom rules run against the parsed Svelte AST in an embedded [Boa](https://boajs.dev/) JS engine, so there's no Node.js dependency and no IPC — just a function call per file.

## Enabling the feature

Custom rules are behind a Cargo feature flag, because bundling the JS engine roughly doubles the binary size. Build with the feature on:

```bash
# install with custom-rules support
cargo install --git https://github.com/tolgaouz/oxvelte.git --features custom-rules

# or from a local checkout
cargo build --release --features custom-rules
```

Without the feature, the `customRules` config option is silently ignored.

## Configuring

Add a `customRules` array to `oxvelte.config.json`. Each entry is a path (or glob with a `*.ext` leaf) relative to the config file's directory.

```json
{
  "rules": {
    "custom/no-div-without-class": "error"
  },
  "customRules": [
    "./rules/no-div-without-class.js",
    "./rules/*.js"
  ]
}
```

Custom rules participate in the config system like any other rule — they can be severity-configured, disabled, or passed options via the `"rules"` map. The `custom/` prefix is a convention, not a requirement, but it keeps your rule names from colliding with future built-ins.

## Rule file structure

Each file exports one rule object. Both `export default` (ESM) and `module.exports =` (CommonJS) work; the loader rewrites either into an internal `__rule` variable before evaluation.

```javascript
export default {
  name: "custom/no-div-without-class",
  run(ctx) {
    // rule body
  },
};
```

Required keys:
- **`name`** — string. Prefix with `custom/` to avoid colliding with built-in rule names. Used when looking up severity / options in config.
- **`run(ctx)`** — function. Called once per linted file. Inspect `ctx.ast` and call `ctx.diagnostic(...)` for each finding.

Other keys (e.g. `meta`, `schema`) are ignored — you can include them for documentation but oxvelte won't read them.

## The `ctx` object

### Properties

| Property | Type | Notes |
|---|---|---|
| `ctx.ast` | object | The parsed Svelte AST — see the [AST reference](#ast-reference). |
| `ctx.source` | string | The raw source text. Use with `node.span` for raw slicing when you need it. |
| `ctx.filePath` | string &#124; null | Absolute path of the file being linted, or `null` when linting from stdin. |
| `ctx.options` | any &#124; null | The second element of your config tuple: `["error", { flag: true }] → { flag: true }`. |
| `ctx.settings` | any &#124; null | The top-level `settings` block from the config file, or `null`. |

### Methods

| Method | Use for |
|---|---|
| `ctx.walk(visitor)` | Recursively visit every template node in document order. The visitor is called with each `TemplateNode`. |
| `ctx.diagnostic(message, span)` | Report an issue at a span. |
| `ctx.diagnosticWithFix(message, span, fix)` | Report with an auto-fix. `fix = { span: { start, end }, replacement: "..." }`. |

`span` is always `{ start: u32, end: u32 }` — byte offsets into the original source. Every AST node has a `span` field, so `node.span` is what you usually pass.

### `ctx.walk` — what it visits

`ctx.walk(fn)` descends into:
- `Element.children`
- `IfBlock.consequent.nodes`, `IfBlock.alternate` (the nested `IfBlock` / `Fragment` for `:else if` / `:else`)
- `EachBlock.body.nodes`, `EachBlock.fallback.nodes`
- `AwaitBlock.pending/then/catch` fragments
- `KeyBlock.body.nodes`, `SnippetBlock.body.nodes`

It does **not** visit script or style content — those are in `ctx.ast.instance`, `ctx.ast.module`, and `ctx.ast.css`, exposed only as raw `{ content, lang, span }` records. Custom rules can inspect the raw text but do not get a typed JS AST.

## AST reference

`ctx.ast` is the JSON projection of oxvelte's internal `SvelteAst`:

```ts
ctx.ast = {
  html: { nodes: TemplateNode[], span },
  instance: { content, lang, span } | null,  // <script> block
  module:   { content, lang, span } | null,  // <script module>
  css:      { content, lang, span } | null,  // <style> block
}
```

### `TemplateNode` variants

Each template node has `type` (the variant name) and `span`. Other fields depend on the variant:

| `type` | Key fields |
|---|---|
| `Text` | `data` (string) |
| `Element` | `name`, `attributes`, `children`, `self_closing` |
| `MustacheTag` | `expression` (raw source string of the JS expression) |
| `RawMustacheTag` | `expression` — emitted from `{@html ...}` |
| `DebugTag` | `identifiers: string[]` — `{@debug foo, bar}` |
| `ConstTag` | `declaration` (raw source string) — `{@const x = 1}` |
| `RenderTag` | `expression` (raw source string) — `{@render foo()}` |
| `Comment` | `data` |
| `IfBlock` | `test` (expression source), `consequent` (`Fragment`), `alternate` (`TemplateNode?` — an `IfBlock` for `:else if`, otherwise wraps a Fragment) |
| `EachBlock` | `expression`, `context`, `index?`, `key?`, `body` (`Fragment`), `fallback` (`Fragment?`) |
| `AwaitBlock` | `expression`, `pending?`, `then?`, `then_binding?`, `catch?`, `catch_binding?` |
| `KeyBlock` | `expression`, `body` (`Fragment`) |
| `SnippetBlock` | `name`, `params`, `body` (`Fragment`) |

### `Element` and `Attribute`

```ts
Element = {
  type: "Element",
  name: string,          // "div", "svelte:head", "MyComponent", "foo.bar"
  attributes: Attribute[],
  children: TemplateNode[],
  self_closing: boolean,
  span,
}

// Attribute is a tagged enum
Attribute =
  | { type: "NormalAttribute", name: string, value: AttributeValue, span }
  | { type: "Spread", span }
  | { type: "Directive", kind: DirectiveKind, name: string,
      modifiers: string[], value: AttributeValue, span }

DirectiveKind =
  "EventHandler" | "Binding" | "Class" | "StyleDirective" | "Use"
  | "Transition" | "In" | "Out" | "Animate" | "Let"

// AttributeValue is a tagged enum
AttributeValue =
  | { type: "Static",     value: string }
  | { type: "Expression", value: string }     // raw JS text of {expr}
  | { type: "Concat",     value: AttributeValuePart[] }
  | { type: "True" }                          // boolean attr without a value
```

### Expressions are strings, not ASTs

Fields like `MustacheTag.expression`, `IfBlock.test`, `EachBlock.expression`, `RenderTag.expression`, and `AttributeValue.Expression.value` are **raw JS source text**, not parsed expressions. Custom rules don't get an `oxc::Expression` for them — that's intentional, since boa would have to re-serialize the AST anyway.

For simple checks (substring match, startsWith), the raw string is fine. For anything deeper, bear in mind you'd be re-implementing a JS parser inside a JS engine, so it's probably the wrong tool — file an issue about elevating the check to a built-in instead.

### Spans and `ctx.source`

`span.start` and `span.end` are byte offsets into `ctx.source` (the original file as read from disk). Slicing is straightforward:

```javascript
const tagText = ctx.source.slice(node.span.start, node.span.end);
```

All spans are absolute to `ctx.source`, including spans inside `instance.content` — script content hasn't been lifted out of the file, so offsets are consistent.

## Auto-fix

```javascript
ctx.diagnosticWithFix(
  "div must have a class attribute",
  node.span,                           // where the diagnostic is reported
  {
    span: { start: node.span.end - 1, end: node.span.end },
    replacement: ' class="todo">',
  }
);
```

The `fix.span` can be any range within the source. `replacement` is a string (can be empty to delete). Fixes are applied in a later pass — order doesn't matter within a single file as long as they don't overlap.

## Complete example

`rules/no-div-without-class.js`:

```javascript
// Require a `class` attribute on every <div>.
export default {
  name: "custom/no-div-without-class",

  run(ctx) {
    ctx.walk((node) => {
      if (node.type !== "Element" || node.name !== "div") return;

      const hasClass = node.attributes.some(
        (a) =>
          (a.type === "NormalAttribute" && a.name === "class") ||
          (a.type === "Directive" && a.kind === "Class"),
      );

      if (!hasClass) {
        ctx.diagnostic("div elements must have a class attribute", node.span);
      }
    });
  },
};
```

`oxvelte.config.json`:

```json
{
  "customRules": ["./rules/no-div-without-class.js"]
}
```

Run:

```bash
oxvelte lint src/
#   src/App.svelte
#     3:1  warning  div elements must have a class attribute  custom/no-div-without-class
```

A runnable copy of this rule lives at [`examples/custom-rules/no-div-without-class.js`](../examples/custom-rules/no-div-without-class.js).

## Limitations

- **No JS/TS semantic model.** `ctx.ast.instance.content` is a string; there's no scope manager, symbol table, or typed expression AST. For rules that need to reason about JS semantics (variable resolution, import tracking, call-graph analysis), add them as built-in Rust rules instead.
- **Synchronous only.** `run(ctx)` is called synchronously per file, with no `await` and no I/O bridge. A rule can't read sibling files, hit the network, or query a language server.
- **Per-file, not cross-file.** Each file is linted in isolation. Rules that need to compare two files (component-to-usage, route-to-schema) aren't expressible here.
- **Boa performance.** Boa is fast enough for a handful of rules across thousands of files, but it's ~100x slower than native Rust rules. If a custom rule becomes hot, port it to a Rust rule.
- **JSON-serialized AST.** Each file's AST is re-serialized to JSON before handing it to the JS engine, so custom rules pay an extra per-file cost. It's amortized well against typical file sizes, but very large files (>100KB of template) take noticeably longer.
