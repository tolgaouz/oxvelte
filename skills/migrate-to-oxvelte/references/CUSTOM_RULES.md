# Custom Rule Migration

Use this reference when a project has local ESLint rules, inline flat-config
plugins, `--rulesdir`, or private plugin packages.

## Classify each custom rule

Read the rule implementation before moving dependencies.

| Rule behavior | Migration target |
|---|---|
| Visits Svelte template nodes, attributes, directives, mustaches, blocks, or comments | oxvelte custom rule |
| Visits JS/TS imports, exports, declarations, calls, identifiers, types, or scopes | oxlint plugin |
| Uses both Svelte template and JS/TS script semantics | split into oxvelte + oxlint rules |
| Needs type-checker services, cross-file analysis, async I/O, or project graph state | keep a minimal ESLint fallback unless the user accepts a lossy migration |

Do not silently drop a custom rule. If exact migration is not possible, preserve
the old rule path and report why.

## Detect local rules

Search for:

- `plugins: { local: { rules: ... } }` or any flat-config inline plugin object
- imports from local rule files in ESLint config
- package names like `eslint-plugin-local`, `eslint-plugin-internal`, or workspace
  plugin packages
- `--rulesdir` in `package.json` scripts
- rule IDs in `rules` config whose namespace is not a known external plugin

Map configured rule IDs to the implementation file before editing config.

## Porting to oxvelte custom rules

Use oxvelte custom rules for template-aware rules. Create rule files under a
project-local folder such as `oxvelte-rules/`.

`oxvelte.config.json`:

```json
{
  "rules": {
    "custom/no-div-without-class": "error"
  },
  "customRules": ["./oxvelte-rules/*.js"]
}
```

Rule shape:

```javascript
export default {
  name: "custom/no-div-without-class",
  run(ctx) {
    ctx.walk((node) => {
      if (node.type !== "Element" || node.name !== "div") return;

      const hasClass = node.attributes.some(
        (attr) =>
          (attr.type === "NormalAttribute" && attr.name === "class") ||
          (attr.type === "Directive" && attr.kind === "Class"),
      );

      if (!hasClass) {
        ctx.diagnostic("div elements must have a class attribute", node.span);
      }
    });
  },
};
```

Available oxvelte custom-rule API:

- `ctx.ast`: parsed Svelte AST
- `ctx.source`: raw file text
- `ctx.filePath`: absolute file path or `null`
- `ctx.options`: rule options from the config tuple
- `ctx.settings`: top-level `settings`
- `ctx.walk(visitor)`: visits template nodes
- `ctx.diagnostic(message, span)`: reports a diagnostic
- `ctx.diagnosticWithFix(message, span, fix)`: reports with an auto-fix

Common AST fields:

- `ctx.ast.html.nodes`: top-level template nodes
- `ctx.ast.instance`: instance `<script>` as raw `{ content, lang, span }`
- `ctx.ast.module`: module script as raw `{ content, lang, span }`
- `ctx.ast.css`: style block as raw `{ content, lang, span }`
- template node variants include `Element`, `Text`, `MustacheTag`,
  `RawMustacheTag`, `Comment`, `IfBlock`, `EachBlock`, `AwaitBlock`,
  `KeyBlock`, and `SnippetBlock`

Expressions are raw strings in oxvelte custom rules, not JS AST nodes. If the
old ESLint rule depends on `context.getScope()`, `parserServices`, identifier
resolution, import graphs, or TypeScript types, use the oxlint plugin path or
keep a minimal ESLint fallback.

Build/install oxvelte with custom rules enabled:

```bash
cargo install --git https://github.com/tolgaouz/oxvelte.git --features custom-rules
```

Without the feature, `customRules` is ignored.

## Porting JS/TS custom rules

Use oxlint plugins for rules that only inspect JavaScript or TypeScript. Check
the current oxlint plugin documentation before implementing because the plugin
API may change. Preserve the old rule behavior and tests as the source of truth.

If the rule cannot be expressed in oxlint today, keep only the minimal ESLint
dependency and script needed for that rule. Do not keep the full Svelte ESLint
stack just for unrelated custom JS/TS rules.

## Preserve configuration behavior

- Keep rule IDs stable when possible.
- Preserve severities: `off`, `warn`, `error`, or numeric equivalents.
- Preserve options from ESLint tuples, e.g. `["error", { "foo": true }]`.
- Move template rule options to `ctx.options`.
- Update disable comments only if a rule ID changes.

## Verification checklist

For every migrated custom rule:

- run the new linter on at least one file that should pass
- run it on at least one file that should report
- compare old ESLint output when the old stack still runs
- verify fixes if the old rule was fixable
- remove ESLint only after custom-rule behavior is accounted for
