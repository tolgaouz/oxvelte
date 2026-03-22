---
name: convert-eslint-config
description: Convert an eslint-plugin-svelte ESLint config to oxvelte.config.json. Use when the user wants to migrate from ESLint to oxvelte, or asks to convert their ESLint config.
argument-hint: <path-to-eslint-config>
---

Convert an ESLint config (with eslint-plugin-svelte rules) to an oxvelte.config.json file.

## Input formats supported

- `.eslintrc.json` / `.eslintrc` — legacy ESLint JSON config
- `eslint.config.json` — flat config as JSON
- `.eslintrc.yaml` / `.eslintrc.yml` — YAML config (read and convert manually)
- `eslint.config.js` / `eslint.config.mjs` — JS flat config (best-effort extraction)
- Resolved config JSON from `npx eslint --print-config file.svelte`

## Steps

1. **Find the config file.** If `$ARGUMENTS` is provided, use that path. Otherwise, look for config files in the project root in this order: `.eslintrc.json`, `.eslintrc`, `eslint.config.json`, `.eslintrc.yaml`, `.eslintrc.yml`, `eslint.config.js`, `eslint.config.mjs`.

2. **Read the config file.**

3. **For JSON configs**, run the oxvelte CLI converter:
   ```bash
   cargo run -- convert-config <path>
   ```
   This extracts only `svelte/*` rules, normalizes severity, and preserves settings.

4. **For JS/MJS configs**, suggest the user resolve it first:
   ```bash
   npx eslint --print-config src/routes/+page.svelte > /tmp/eslint-resolved.json
   cargo run -- convert-config /tmp/eslint-resolved.json --write
   ```
   Alternatively, read the JS file and manually extract any `svelte/*` rule entries.

5. **For YAML configs**, read the YAML, identify `svelte/*` rules under the `rules:` key, and construct the equivalent JSON. The format mapping is:
   - `"off"` / `0` → `"off"`
   - `"warn"` / `1` → `"warn"`
   - `"error"` / `2` → `"error"`
   - `["error", { options }]` → `["error", { options }]`

6. **Handle ESLint extends/presets.** If the config uses `extends: ["plugin:svelte/recommended"]` or `...svelte.configs['flat/recommended']`, note that oxvelte's `--all-rules` flag or default recommended set covers these. Only explicit rule overrides need to be in oxvelte.config.json.

7. **Handle settings.** Preserve `settings.svelte` (especially `kit.files.routes`) in the output.

8. **Write the output** to `oxvelte.config.json` in the project root.

9. **Show the user** what was converted — list the rules and their severities.

## Output format

```json
{
  "rules": {
    "svelte/no-at-html-tags": "error",
    "svelte/button-has-type": ["warn", { "button": false }],
    "svelte/indent": "off"
  },
  "settings": {
    "svelte": {
      "kit": { "files": { "routes": "src/routes" } }
    }
  }
}
```

## Rules to skip

- Non-svelte rules (e.g., `no-console`, `@typescript-eslint/*`) — not handled by oxvelte
- `svelte/valid-compile` — handled by the Svelte compiler itself
- `svelte/no-unused-svelte-ignore` — requires Svelte compiler integration

## Notes

- If the user has eslint-plugin-svelte's recommended preset enabled with no overrides, tell them they don't need an oxvelte.config.json at all — oxvelte's default recommended rules match.
- The `svelte/comment-directive` and `svelte/system` rules are internal to eslint-plugin-svelte and don't need conversion.
