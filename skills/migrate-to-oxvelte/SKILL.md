---
name: migrate-to-oxvelte
description: >
  Migrate a Svelte project from eslint-plugin-svelte to oxvelte, a drop-in
  replacement Svelte linter written in Rust (4-25x faster). Detects your ESLint
  config, converts svelte rules to oxvelte.config.json, updates package.json
  scripts, removes unused ESLint dependencies, and verifies the migration.
  Use when a user wants to switch from eslint-plugin-svelte to oxvelte.
license: MIT
metadata:
  author: tolgaouz
  version: "1.0"
---

# Migrate from eslint-plugin-svelte to oxvelte

You are helping the user migrate their Svelte project from `eslint-plugin-svelte` (ESLint-based) to **oxvelte** — a drop-in replacement Svelte linter written in Rust that is 4-25x faster. Same rule names, same options, same diagnostics.

Follow these steps in order. After each major step, briefly report what you did.

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

## Step 3: Update package.json

Remove from `devDependencies`:
- `eslint-plugin-svelte`
- `svelte-eslint-parser`

**Do NOT remove** general ESLint packages unless the user confirms they want to drop ESLint entirely.

Update lint scripts. Recommended setup:

```json
{
  "scripts": {
    "lint": "oxlint && oxvelte lint src/",
    "lint:fix": "oxlint --fix && oxvelte lint --fix src/"
  }
}
```

With TypeScript (if `tsconfig.json` exists):

```json
{
  "scripts": {
    "lint": "oxlint --tsconfig tsconfig.json && oxvelte lint src/",
    "lint:fix": "oxlint --fix --tsconfig tsconfig.json && oxvelte lint --fix src/"
  }
}
```

If keeping ESLint for non-Svelte rules:

```json
{
  "scripts": {
    "lint": "eslint src/ && oxvelte lint src/"
  }
}
```

Add `oxlint` to devDependencies if fully replacing ESLint and it's not already present.

## Step 4: Clean up ESLint config

**If fully replacing ESLint** (no non-Svelte plugins remain):
- Delete the ESLint config file
- Remove all ESLint devDependencies (`eslint`, `@eslint/js`, `typescript-eslint`, etc.)
- Delete `.eslintignore` if it exists

**If keeping ESLint for non-Svelte rules:**
- Remove `eslint-plugin-svelte` from plugins
- Remove all `svelte/*` rule entries
- Remove the Svelte parser config (`parser: 'svelte-eslint-parser'`)
- Remove Svelte-specific overrides blocks
- Keep all non-Svelte rules and plugins

## Step 5: Comment directives

Tell the user: existing `eslint-disable` comments for Svelte rules will continue to work with oxvelte. No find-and-replace needed.

Supported formats:
- `/* eslint-disable svelte/rule-name */`
- `// eslint-disable-next-line svelte/rule-name`
- `<!-- eslint-disable svelte/rule-name -->`
- `<!-- svelte-ignore rule-name -->`
- `/* oxvelte-disable */` (oxvelte-native format)

## Step 6: Install oxvelte

```bash
cargo install --git https://github.com/tolgaouz/oxvelte.git
```

Or from source:

```bash
git clone https://github.com/tolgaouz/oxvelte.git
cd oxvelte && cargo build --release
```

## Step 7: Verify

Run oxvelte on the project:

```bash
oxvelte lint src/
```

If there are issues, help the user adjust the config. If the user had an ESLint config file, mention they can also use the built-in converter:

```bash
oxvelte migrate <old-eslint-config> --write
```
