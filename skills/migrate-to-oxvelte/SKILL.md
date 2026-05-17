---
name: migrate-to-oxvelte
description: >
  Migrate a Svelte project from ESLint/eslint-plugin-svelte to the default
  oxc stack: oxlint plus oxvelte. Detects the current ESLint config, converts
  svelte rules to oxvelte.config.json, ports local custom rules when possible,
  updates package.json scripts, removes obsolete dependencies, and verifies the
  migration. Use when a user wants to switch from eslint-plugin-svelte to
  oxvelte, drop ESLint for Svelte files, or move to an oxc/oxlint + oxvelte lint
  stack.
license: MIT
metadata:
  author: tolgaouz
  version: "1.2"
---

# Migrate from ESLint to oxlint + oxvelte

You are helping the user migrate their Svelte project from ESLint and
`eslint-plugin-svelte` to the default oxc linting stack:

- `oxlint` for JS/TS/import/general rules
- `oxvelte` for Svelte-specific `svelte/*` rules

When the user says "oxc + oxvelte", interpret that as the practical stack
`oxlint + oxvelte`: `oxlint` handles general JS/TS/import rules, and `oxvelte`
handles `svelte/*` rules.

Follow these steps in order. After each major step, briefly report what you did.

## Step 0: Choose the migration mode

Default to Mode B. Pick Mode A only when the user explicitly asks to keep ESLint
or when a custom rule cannot be safely ported in the current turn.

### Mode A: Svelte-only migration

Use this only when the user asks to migrate only `eslint-plugin-svelte`, keep
ESLint for non-Svelte rules, or pause before fully replacing ESLint.

Outcome:
- Keep ESLint for JS/TS/general rules.
- Remove only Svelte-specific ESLint pieces.
- Add or keep `oxvelte`.
- Keep non-Svelte ESLint dependencies and config.

### Mode B: Full oxlint + oxvelte migration

Use this by default, and always when the user asks to:
- "completely switch"
- "drop ESLint"
- "replace ESLint"
- "move to oxc + oxvelte"
- "move to oxlint + oxvelte"
- "use the full oxvelte/oxc stack"

Outcome:
- Replace the general ESLint stack with `oxlint`.
- Replace `eslint-plugin-svelte` with `oxvelte`.
- Remove ESLint config and obsolete ESLint dependencies when no longer needed.
- Update package scripts to run `oxlint` and `oxvelte`.
- Port local custom rules to oxvelte custom rules or oxlint plugins when
  possible.

Do not leave ESLint behind just because custom rules exist. First classify and
port them. Preserve a minimal ESLint path only for custom rules that rely on
unsupported behavior such as type-checker services, cross-file state, async I/O,
or rule APIs that cannot be mapped safely.

## Step 1: Detect the current ESLint setup

Find the ESLint config file. Check for (in priority order):

- `eslint.config.js` / `eslint.config.mjs` / `eslint.config.ts` (flat config)
- `.eslintrc.json` / `.eslintrc.js` / `.eslintrc.cjs` / `.eslintrc.yml` / `.eslintrc`

Read the config file and `package.json`. Identify:

1. Which `svelte/*` rules are explicitly configured (enabled, disabled, or with options)
2. Which non-Svelte ESLint plugins are in use (`@eslint/js`, `typescript-eslint`, `eslint-plugin-import`, etc.)
3. Whether `eslint-plugin-svelte` is in `devDependencies`
4. Any svelte-related settings (e.g. `settings.svelte.kit.files.routes`)
5. Existing lint scripts in `package.json`
6. Whether the user request implies Mode A or Mode B
7. Which ESLint plugins map cleanly to oxlint, and which do not
8. Local/custom ESLint rules and plugins, including:
   - flat-config inline plugins: `plugins: { local: { rules: ... } }`
   - local plugin packages such as `eslint-plugin-local` or workspace packages
   - direct local imports from paths like `./eslint-rules/*`
   - legacy `--rulesdir` usage in scripts
   - rule IDs in project/private namespaces that are not standard packages

Report a summary of what you found.

## Step 2: Generate oxvelte.config.json

If the user has explicit `svelte/*` rule overrides beyond the recommended set, create `oxvelte.config.json`. If they only use the recommended preset with no overrides, skip this — oxvelte defaults match eslint-plugin-svelte's `flat/recommended`.

Config format:

```json
{
  "rules": {
    "svelte/rule-name": "error",
    "svelte/rule-with-options": ["warn", { "option": "value" }]
  },
  "settings": {
    "svelte": {
      "kit": {
        "files": { "routes": "src/routes" }
      }
    }
  }
}
```

Conversion rules:
- Only include `svelte/*` rules — non-svelte rules belong in oxlint config
- Severity: `"off"` / `"warn"` / `"error"` (or `0` / `1` / `2`) — same as ESLint
- Options format: `["error", { ...options }]` — same as ESLint
- Preserve any `settings.svelte.kit.files.routes` value

If the eslint config enables `valid-compile` or `no-unused-svelte-ignore`, note in a comment that these are handled by the Svelte compiler at build time and are intentionally excluded from oxvelte.

See `references/RULES.md` for the complete list of supported rules.

## Step 3: Migrate custom rules

If no local/custom rules are present, say so and continue.

If local/custom rules are present, load `references/CUSTOM_RULES.md` and port
them before deleting ESLint:

- Template/Svelte rules become oxvelte custom rules and are added to
  `oxvelte.config.json` via `customRules`.
- JS/TS-only rules become oxlint plugins when the rule can be mapped to oxlint's
  plugin API.
- Mixed template + script rules should be split: the template part goes to
  oxvelte and the JS/TS part goes to oxlint.
- Keep the same rule IDs and severities when possible. If a rename is necessary,
  update all config entries and disable comments.
- Preserve options from ESLint rule tuples as oxvelte `ctx.options` or the
  equivalent oxlint plugin configuration.
- Add a short migration note only when a rule cannot be ported safely.

For oxvelte custom rules, install/build oxvelte with custom-rule support:

```bash
cargo install --git https://github.com/tolgaouz/oxvelte.git --features custom-rules
```

Do not silently drop a custom rule. If a rule cannot be ported, leave ESLint in
place only for that rule and report the blocker clearly.

## Step 4: Update package.json

Remove from `devDependencies` in both modes:
- `eslint-plugin-svelte`
- `svelte-eslint-parser`

In Mode A, do not remove general ESLint packages.

In Mode B, remove general ESLint packages that are being replaced by oxlint:
- `eslint`
- `@eslint/js`
- `typescript-eslint`
- `@typescript-eslint/eslint-plugin`
- `@typescript-eslint/parser`
- `eslint-plugin-import`
- `eslint-plugin-n`
- `eslint-plugin-unicorn`
- other ESLint packages only when their rules are intentionally being dropped
  or have an oxlint equivalent in the project context.

Add `oxlint` to `devDependencies` if Mode B is selected and it is not already present.

Update lint scripts.

Mode B recommended setup:

```json
{
  "scripts": {
    "lint": "oxlint && oxvelte lint src/",
    "lint:fix": "oxlint --fix && oxvelte lint --fix src/"
  }
}
```

Mode B with TypeScript (if `tsconfig.json` exists):

```json
{
  "scripts": {
    "lint": "oxlint --tsconfig tsconfig.json && oxvelte lint src/",
    "lint:fix": "oxlint --fix --tsconfig tsconfig.json && oxvelte lint --fix src/"
  }
}
```

Mode A setup, keeping ESLint for non-Svelte rules:

```json
{
  "scripts": {
    "lint": "eslint src/ && oxvelte lint src/"
  }
}
```

Preserve project-specific lint path globs when possible. For example, if the
existing script lints `src routes packages`, keep equivalent paths for
`oxvelte lint`.

## Step 5: Clean up ESLint config

Mode B cleanup:
- Delete the ESLint config file
- Remove all ESLint devDependencies (`eslint`, `@eslint/js`, `typescript-eslint`, etc.)
- Delete `.eslintignore` if it exists
- If the project has ignore patterns that still matter, move them to the
  appropriate oxlint ignore/config mechanism or preserve them in package scripts

Mode A cleanup:
- Remove `eslint-plugin-svelte` from plugins
- Remove all `svelte/*` rule entries
- Remove the Svelte parser config (`parser: 'svelte-eslint-parser'`)
- Remove Svelte-specific overrides blocks
- Keep all non-Svelte rules and plugins

For Mode B, do not leave a dead ESLint config behind unless unresolved ESLint
plugins still need a follow-up decision.

## Step 6: Comment directives

Tell the user: existing `eslint-disable` comments for Svelte rules will continue to work with oxvelte. No find-and-replace needed.

Supported formats:
- `/* eslint-disable svelte/rule-name */`
- `// eslint-disable-next-line svelte/rule-name`
- `<!-- eslint-disable svelte/rule-name -->`
- `<!-- svelte-ignore rule-name -->`
- `/* oxvelte-disable */` (oxvelte-native format)

When custom rule IDs change, update affected disable comments in source files.

## Step 7: Install tools

For Mode B, install `oxlint` with the project's package manager:

```bash
pnpm add -D oxlint
```

Use `npm install -D oxlint`, `yarn add -D oxlint`, or `bun add -d oxlint` if
that is the project's package manager.

```bash
cargo install --git https://github.com/tolgaouz/oxvelte.git
```

If custom oxvelte rules were generated, use:

```bash
cargo install --git https://github.com/tolgaouz/oxvelte.git --features custom-rules
```

Or from source:

```bash
git clone https://github.com/tolgaouz/oxvelte.git
cd oxvelte && cargo build --release
```

## Step 8: Verify

Run oxvelte on the project:

```bash
oxvelte lint src/
```

For Mode B, also run oxlint:

```bash
oxlint --tsconfig tsconfig.json
```

Omit `--tsconfig tsconfig.json` when the project has no `tsconfig.json`.

If custom rules were ported, verify them against at least one fixture or source
file that should trigger the rule and one that should not. Compare old ESLint
output when possible before removing ESLint.

If there are issues, help the user adjust the config. If the user had an ESLint config file, mention they can also use the built-in converter:

```bash
oxvelte migrate <old-eslint-config> --write
```
