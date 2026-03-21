# Oxvelte — Program

> A Rust-based Svelte parser and linter powered by oxc. The agent builds, tests, and iterates
> on the codebase autonomously. The human edits this file.

---

## Overview

Oxvelte is a high-performance Svelte parser and linter written in Rust. It reuses oxc's
JavaScript/TypeScript parser, formatter (oxcfmt), and semantic analyzer for script blocks,
and implements a custom Svelte template parser that produces a unified AST. Lint rules are
ported from eslint-plugin-svelte so that the entire Svelte linting experience is available
as a single, fast native binary — no Node.js required.

### Key dependencies (Rust crates)

| Crate              | Purpose                                                |
|--------------------|--------------------------------------------------------|
| `oxc`              | Umbrella crate — re-exports parser, semantic, allocator, span, AST, codegen. Use `oxc::{parser, semantic, allocator, span, ast}` in Rust code. |
| `oxc_diagnostics`  | Structured error reporting (miette-based) — published separately |

The `oxc` crate is published to crates.io and acts like a single dependency that gives
you access to all of oxc's tools via feature flags. In `Cargo.toml`:
```toml
oxc = { version = "*", features = ["full", "serialize"] }
```
In Rust code:
```rust
use oxc::{allocator::Allocator, parser::Parser, span::Span, semantic::SemanticBuilder};
```

**Important: oxc is a dependency, not our code.** It is downloaded from crates.io and
compiled into our binary. Think of it like `node_modules` — you import from it, you call
its APIs, but you never open its source files and edit them. All of our code lives under
`src/`. If oxc's API doesn't do exactly what you need, you write adapter
code in `src/` — you do not modify oxc.

### Reference repositories (cloned during setup)

| Repo                                              | What we take from it                            |
|---------------------------------------------------|-------------------------------------------------|
| `https://github.com/sveltejs/svelte`              | Parser test fixtures, AST shape reference        |
| `https://github.com/sveltejs/eslint-plugin-svelte`| Lint rule logic, test fixtures, rule metadata    |

---

## Phase 1 — Setup (one-time, collaborative with human)

Work with the human to:

1. **Agree on a run tag**: propose a tag based on today's date (e.g. `mar20`).
   The branch `oxvelte/<tag>` must not already exist — this is a fresh run.

2. **Create the branch**:
   ```bash
   git checkout -b oxvelte/<tag> main
   ```

3. **Read the in-scope files**: The repo is small at the start. Read every file for full
   context:
   - `README.md` — repository context
   - `Cargo.toml` — dependencies and build config (do not modify)
   - `src/lib.rs` — module declarations + integration tests (agent modifies)
   - `src/main.rs` — CLI entry point (agent modifies)
   - `src/ast.rs` — Svelte AST type definitions (agent modifies)
   - `src/parser/mod.rs` — Svelte template parser (agent modifies)
   - `src/linter/mod.rs` — lint rule runner (agent modifies)
   - `src/linter/rules/mod.rs` — rule registry (agent modifies)
   - `prepare.sh` — clones reference repos + copies test fixtures (do not modify)
   - `verify_fixtures.sh` — checksums fixtures to prevent tampering (do not modify)

4. **Verify reference repos are cloned**: Check that `./vendor/svelte` and
   `./vendor/eslint-plugin-svelte` exist AND that `vendor/` is read-only.
   If not, tell the human to run:
   ```bash
   bash prepare.sh
   ```
   This also generates `.fixtures.sha256` and installs a git pre-commit hook.

5. **Verify fixture integrity**: Run `bash verify_fixtures.sh --verify` and confirm it
   passes. This is your baseline — all future commits will be gated on this check.

6. **Initialize results.tsv**: Create `results.tsv` with just the header row:
   ```
   experiment	status	parser_tests_pass	parser_tests_total	lint_rules_impl	lint_tests_pass	lint_tests_total	notes
   ```
   The baseline will be recorded after the first run.

7. **Confirm and go**: Confirm setup looks good. Once you get confirmation, kick off
   experimentation.

---

## Phase 2 — Experimentation (autonomous, indefinite)

Each experiment is one atomic unit of progress on the parser or linter.
The loop runs on the branch created in Phase 1.

### Experiment loop

```
while true:
    1. Pick an objective (see "What to work on" below)
    2. Implement the change by editing files in src/
    3. Build:  cargo build 2>&1 | tee build.log
       - If it fails, read the errors, fix, retry. After 3 failed attempts, revert and skip.
    4. Test:   cargo test > test.log 2>&1
       - Parse test.log for pass/fail counts.
    5. Verify: bash verify_fixtures.sh --verify
       - This MUST pass. If it fails, you modified a test fixture. Revert immediately.
    6. Record results in results.tsv
    7. If tests improved or stayed the same AND no regressions AND verify passed:
         git add -A && git commit -m "exp: <short description>"
       If tests regressed OR verify failed:
         git checkout -- .   (revert everything)
    8. Loop
```

**CRITICAL**: Step 5 is non-negotiable. The `verify_fixtures.sh` script checksums every
test fixture against the vendor originals. If you modified any fixture file (even
accidentally), it will fail and you must revert. The point of this project is to make
the *implementation* pass the *existing* tests — not to make the tests pass the
implementation. A git pre-commit hook also enforces this, but you should catch it
earlier in the loop to avoid wasted work.

### What to work on

Work through these objectives **roughly in order**, but use your judgment. If you get
stuck on one, move to the next and come back.

#### A. Svelte Template Parser (`src/parser/`)

The parser must handle the full Svelte template language. Build it incrementally:

1. **Basic HTML**: elements, attributes, text nodes, comments, void elements, self-closing.
2. **Mustache tags**: `{expression}`, `{@html expr}`, `{@debug vars}`, `{@const decl}`.
3. **Logic blocks**: `{#if}` / `{:else if}` / `{:else}` / `{/if}`,
   `{#each}` / `{:else}` / `{/each}`,
   `{#await}` / `{:then}` / `{:catch}` / `{/await}`,
   `{#key}` / `{/key}`.
4. **Snippet blocks** (Svelte 5): `{#snippet name(params)}` / `{/snippet}`, `{@render snippet(args)}`.
5. **Special elements**: `<svelte:self>`, `<svelte:component>`, `<svelte:element>`,
   `<svelte:window>`, `<svelte:document>`, `<svelte:body>`, `<svelte:head>`,
   `<svelte:options>`, `<svelte:fragment>`, `<svelte:boundary>`.
6. **Directives**: `on:event`, `bind:prop`, `class:name`, `style:prop`, `use:action`,
   `transition:fn`, `in:fn`, `out:fn`, `animate:fn`, `let:var`.
7. **Script and style blocks**: `<script>`, `<script context="module">`,
   `<script module>` (Svelte 5), `<style>`. Hand off JS/TS content to `oxc::parser::Parser`.
8. **Svelte 5 runes**: `$state`, `$derived`, `$effect`, `$props`, `$bindable`, `$inspect`,
   `$host`. These are parsed inside script blocks by oxc — ensure the AST captures them.

**Metric**: number of passing parser snapshot tests from `fixtures/parser/`.
Compare your parser's JSON AST output against the expected `.json` files. Use
`cargo test parser` to run.

#### B. AST Definitions (`src/ast.rs`)

Define Rust types that mirror the Svelte AST. Cross-reference:
- `vendor/svelte/packages/svelte/src/compiler/types/template.d.ts`
- The svelte-eslint-parser AST docs

All AST nodes must carry `Span` for source locations. Use `oxc::allocator` arena allocation
where appropriate.

#### C. Lint Rules (`src/linter/`)

Port every rule from eslint-plugin-svelte. The full list:

**Possible Errors (⭐ = recommended)**
- `svelte/infinite-reactive-loop` ⭐
- `svelte/no-dom-manipulating` ⭐
- `svelte/no-dupe-else-if-blocks` ⭐
- `svelte/no-dupe-on-directives` ⭐
- `svelte/no-dupe-style-properties` ⭐
- `svelte/no-dupe-use-directives` ⭐
- `svelte/no-not-function-handler` ⭐
- `svelte/no-object-in-text-mustaches` ⭐
- `svelte/no-raw-special-elements` ⭐ 🔧
- `svelte/no-reactive-reassign` ⭐
- `svelte/no-shorthand-style-property-overrides` ⭐
- `svelte/no-store-async` ⭐
- `svelte/no-top-level-browser-globals`
- `svelte/no-unknown-style-directive-property` ⭐
- `svelte/prefer-svelte-reactivity` ⭐
- `svelte/require-store-callbacks-use-set-param` 💡
- `svelte/require-store-reactive-access` ⭐ 🔧
- `svelte/valid-compile`
- `svelte/valid-style-parse`

**Security Vulnerability**
- `svelte/no-at-html-tags` ⭐
- `svelte/no-target-blank`

**Best Practices**
- `svelte/block-lang` 💡
- `svelte/button-has-type`
- `svelte/no-add-event-listener` 💡
- `svelte/no-at-debug-tags` ⭐ 💡
- `svelte/no-ignored-unsubscribe`
- `svelte/no-immutable-reactive-statements` ⭐
- `svelte/no-inline-styles`
- `svelte/no-inspect` ⭐
- `svelte/no-reactive-functions` ⭐ 💡
- `svelte/no-reactive-literals` ⭐ 💡
- `svelte/no-svelte-internal` ⭐
- `svelte/no-unnecessary-state-wrap` ⭐ 💡
- `svelte/no-unused-class-name`
- `svelte/no-unused-props` ⭐
- `svelte/no-unused-svelte-ignore` ⭐
- `svelte/no-useless-children-snippet` ⭐
- `svelte/no-useless-mustaches` ⭐ 🔧
- `svelte/prefer-const` 🔧
- `svelte/prefer-destructured-store-props` 💡
- `svelte/prefer-writable-derived` ⭐ 💡
- `svelte/require-each-key` ⭐
- `svelte/require-event-dispatcher-types` ⭐
- `svelte/require-optimized-style-attribute`
- `svelte/require-stores-init`
- `svelte/valid-each-key` ⭐

**Stylistic Issues**
- `svelte/consistent-selector-style`
- `svelte/derived-has-same-inputs-outputs` 💡
- `svelte/first-attribute-linebreak` 🔧
- `svelte/html-closing-bracket-new-line` 🔧
- `svelte/html-closing-bracket-spacing` 🔧
- `svelte/html-quotes` 🔧
- `svelte/html-self-closing` 🔧
- `svelte/indent` 🔧
- `svelte/max-attributes-per-line` 🔧
- `svelte/max-lines-per-block`
- `svelte/mustache-spacing` 🔧
- `svelte/no-extra-reactive-curlies` 💡
- `svelte/no-restricted-html-elements`
- `svelte/no-spaces-around-equal-signs-in-attribute` 🔧
- `svelte/prefer-class-directive` 🔧
- `svelte/prefer-style-directive` 🔧
- `svelte/require-event-prefix`
- `svelte/shorthand-attribute` 🔧
- `svelte/shorthand-directive` 🔧
- `svelte/sort-attributes` 🔧
- `svelte/spaced-html-comment` 🔧

**Extension Rules**
- `svelte/no-inner-declarations` ⭐
- `svelte/no-trailing-spaces` 🔧

**SvelteKit**
- `svelte/no-export-load-in-svelte-module-in-kit-pages` ⭐
- `svelte/no-navigation-without-resolve` ⭐
- `svelte/valid-prop-names-in-kit-pages` ⭐

**Experimental**
- `svelte/experimental-require-slot-types`
- `svelte/experimental-require-strict-events`

**System**
- `svelte/comment-directive` ⭐
- `svelte/system` ⭐

**Total: ~70 rules.**

For each rule:
1. Read the original JS implementation in `vendor/eslint-plugin-svelte/packages/eslint-plugin-svelte/src/rules/<rule-name>.ts`
2. Read the tests in `vendor/eslint-plugin-svelte/packages/eslint-plugin-svelte/tests/src/rules/<rule-name>.ts`
3. The test fixture `.svelte` files are already in `fixtures/linter/<rule-name>/` (read-only, do not copy or modify them)
4. Implement the rule in Rust under `src/linter/rules/<rule_name>.rs`
5. Add snapshot tests that match the eslint-plugin-svelte expected output

**Metric**: number of lint rules implemented × passing test cases.
Use `cargo test linter` to run.

#### D. CLI (`src/main.rs`)

The CLI should support:
```
oxvelte lint [paths...]         # Run linter
oxvelte parse [file]            # Dump AST as JSON
oxvelte fmt [paths...]          # Format (delegates JS/TS to oxcfmt)
oxvelte check [paths...]        # Parse + lint + type-aware checks
```

#### E. Integration Tests (`tests/`)

End-to-end tests that run the CLI binary on `.svelte` files and assert output.

---

## Phase 3 — Hardening (later, agent + human)

- Fuzz the parser with `cargo-fuzz` / `afl`
- Benchmark against the Svelte compiler's own parser
- WASM build for editor integration (`wasm-pack`)
- npm wrapper package so users can `npx oxvelte lint`

---

## Constraints

### The one rule that matters most

**The agent may ONLY create or modify files inside the `src/` directory.** Everything
outside `src/` is off-limits. This is the single, simple boundary:

```
src/                     ← YOU WRITE CODE HERE. This is the ONLY place you touch.
  lib.rs                 ← module declarations + integration tests
  main.rs                ← CLI entry point
  ast.rs                 ← Svelte AST type definitions
  parser/                ← Svelte template parser
  linter/rules/          ← lint rule implementations

Everything else          ← HANDS OFF. Read, but never write.
  Cargo.toml             ← dependency manifest (like package.json)
  fixtures/              ← read-only test data from reference repos
  vendor/                ← cloned svelte + eslint-plugin-svelte repos
  prepare.sh             ← setup script
  verify_fixtures.sh     ← integrity checker
  program.md             ← this file (human-edited only)
  ~/.cargo/registry/     ← downloaded dependencies (like node_modules)
```

If a file is not under `src/`, do not create it, modify it, or delete it.
This includes dependencies (`oxc`, `serde`, etc.) which live in `~/.cargo/registry/`.
They are third-party libraries — you import from them, you never edit them.
If an API doesn't do what you need, write adapter code in `src/`.

### Other constraints

- **Build must pass**: never commit if `cargo build` fails.
- **Tests must not regress**: if your change causes previously-passing tests to fail, revert.
- **Fixture verification must pass**: run `bash verify_fixtures.sh` before every commit.
  The `fixtures/` directory is read-only and checksummed against `vendor/` originals.
- **Timeout**: No process should take more than 2 minutes. `cargo build` and `cargo test`
  should complete in seconds for a project this size. If any process takes longer than
  2 minutes, **kill it immediately** (`pkill -f "cargo test"`) and investigate for
  performance issues (infinite loops, infinite recursion, runaway allocation) before
  retrying. Do NOT retry without fixing the root cause first — a hanging process means
  there is a bug in your code.
- **Crashes**: if a build or test crashes, use your judgment:
  - If it's a simple fix (typo, missing import, borrow checker issue), fix and retry.
  - If the approach is fundamentally broken, revert and try something else.
- **NEVER STOP**: once the experiment loop has begun (after Phase 1), do NOT pause to ask
  the human if you should continue. The human might be asleep or away from the computer
  and expects you to continue working indefinitely until manually stopped. You are
  autonomous. If you run out of ideas, re-read the reference repos for new angles, try
  combining partial implementations, or start on a different subsystem.

---

## Metrics & Logging

After every experiment, append a row to `results.tsv`:

```
experiment	status	parser_tests_pass	parser_tests_total	lint_rules_impl	lint_tests_pass	lint_tests_total	notes
baseline	pass	0	423	0	0	1847	initial scaffold
exp-001	pass	12	423	0	0	1847	basic HTML elements
exp-002	fail	-	-	-	-	-	attempted mustache tags, build error
exp-003	pass	38	423	0	0	1847	mustache tags working
```

Do NOT commit `results.tsv` — leave it untracked by git.

Parse test counts from `cargo test` output using:
```bash
grep -E "test result:" test.log
```

---

## Architecture Notes for the Agent

### How the parser works

```
┌──────────────────────────────────────────────────────┐
│                   .svelte source                     │
└──────────────────┬───────────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────────┐
│              oxvelte::parser::parse()                 │
│                                                      │
│  1. Scan for <script>, <style>, template regions     │
│  2. Template region → custom Svelte template parser  │
│  3. <script> content → oxc::parser::Parser          │
│  4. <style> content → stored as raw CSS (for now)    │
│  5. Stitch into unified SvelteAst                    │
└──────────────────┬───────────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────────┐
│                  SvelteAst                           │
│                                                      │
│  ├── html: Fragment (template nodes)                 │
│  ├── instance: Option<Script> (oxc Program)          │
│  ├── module: Option<Script> (oxc Program)            │
│  └── css: Option<Style>                              │
└──────────────────────────────────────────────────────┘
```

### How the linter works

```
┌──────────────┐     ┌───────────────────┐     ┌──────────────┐
│ parser        │────▶│ oxc::semantic      │────▶│ linter        │
│  (parse)     │     │  (build scopes,   │     │ (run rules)  │
│              │     │   symbols, types)  │     │              │
└──────────────┘     └───────────────────┘     └──────────────┘
                                                       │
                                                       ▼
                                               ┌──────────────┐
                                               │ Diagnostics   │
                                               │(oxc_diagnostics)│
                                               └──────────────┘
```

For each lint rule:
1. The rule receives a `LintContext` containing the full `SvelteAst` and `Semantic`.
2. It walks the relevant AST nodes (template, script, or both).
3. It calls `ctx.diagnostic(...)` for each violation found.
4. Rules marked 🔧 must also provide a `Fix` with replacement text + span.

### Reference file locations

After running `prepare.sh`, these paths are available:

```
fixtures/parser/modern/          ← copied from vendor, READ-ONLY
  └── <test-name>/
      ├── input.svelte
      └── output.json

fixtures/parser/legacy/          ← copied from vendor, READ-ONLY
  └── <test-name>/
      ├── input.svelte
      └── output.json

fixtures/linter/                 ← copied from vendor, READ-ONLY
  └── <rule-name>/
      ├── valid/
      └── invalid/

vendor/svelte/packages/svelte/src/compiler/types/
  └── template.d.ts              ← AST shape reference (read to understand, don't modify)

vendor/eslint-plugin-svelte/packages/eslint-plugin-svelte/src/rules/
  └── <rule-name>.ts             ← original JS rule impl (read to port, don't modify)

vendor/eslint-plugin-svelte/packages/eslint-plugin-svelte/tests/src/rules/
  └── <rule-name>.ts             ← original test file (read for context, don't modify)
```

### Understanding the lint test fixtures

The fixtures in `fixtures/linter/<rule-name>/` are NOT just plain `.svelte` files. They
follow a specific convention from eslint-plugin-svelte's test harness. You MUST understand
this convention to port tests correctly.

#### Directory structure per rule

```
fixtures/linter/<rule-name>/
  ├── valid/                         ← files that should produce ZERO diagnostics
  │   ├── simple-case.svelte
  │   ├── simple-case-config.json    ← optional: ESLint config for this specific file
  │   ├── with-options.svelte
  │   ├── with-options-config.json
  │   └── _config.json               ← optional: default config for ALL files in valid/
  │
  └── invalid/                       ← files that SHOULD produce diagnostics
      ├── bad-code.svelte
      ├── bad-code-config.json       ← optional: ESLint config for this specific file
      ├── bad-code-errors.json       ← EXPECTED errors (line, column, message, etc.)
      ├── bad-code-output.svelte     ← EXPECTED auto-fix output (for fixable rules)
      └── _config.json               ← optional: default config for ALL files in invalid/
```

#### What the config files contain

The `*-config.json` and `_config.json` files are ESLint config objects. They typically
contain one or more of:

**1. Rule options** — the most common. Many rules accept configuration:
```json
{
  "rules": {
    "svelte/html-quotes": ["error", { "prefer": "single" }]
  }
}
```
→ For oxvelte: this means the rule needs a `RuleConfig` struct with these options.
  The test should pass the same options to the rule.

**2. Rule severity override**:
```json
{
  "rules": {
    "svelte/no-at-html-tags": "off"
  }
}
```
→ For oxvelte: skip this file if the rule is disabled, or test that it's suppressed.

**3. Settings (plugin-level config)**:
```json
{
  "settings": {
    "svelte": {
      "ignoreWarnings": ["@typescript-eslint/no-unsafe-assignment"],
      "compileOptions": { "postcss": false },
      "kit": { "files": { "routes": "src/routes" } }
    }
  }
}
```
→ For oxvelte: these map to a `SvelteSettings` struct that rules can read from context.
  The `kit.files.routes` setting affects SvelteKit-specific rules.

**4. Parser options / language options**:
```json
{
  "languageOptions": {
    "parserOptions": {
      "svelteConfig": { "compilerOptions": { "runes": true } }
    }
  }
}
```
→ For oxvelte: this tells the parser whether to expect Svelte 5 runes mode.

**5. Test control flags** (not linter config, just test harness):
```json
{
  "only": true
}
```
→ For oxvelte: ignore this. It's a mocha `.only()` flag for debugging, not rule config.

#### What the errors files contain

The `*-errors.json` files describe the expected diagnostics for invalid test cases:
```json
[
  {
    "message": "Unexpected `{@html}`.",
    "line": 3,
    "column": 1,
    "endLine": 3,
    "endColumn": 19
  }
]
```
→ For oxvelte: your rule's diagnostic output must match these locations and messages.

#### What the output files contain

The `*-output.svelte` files show what the file should look like AFTER auto-fix:
```svelte
<!-- before (in the .svelte file): -->
<img src="foo.png"></img>
<!-- after (in the -output.svelte file): -->
<img src="foo.png" />
```
→ For oxvelte: your rule's `Fix` must produce this exact output when applied.

#### How to handle this when porting a rule

1. **Read the `_config.json`** in the fixture directory first — it tells you what
   options the rule supports and what the default test configuration is.
2. **Read each `*-config.json`** next to individual test files — these override the
   defaults for specific test cases. If a test file has a config, the rule behavior
   may differ from the default.
3. **Design your `Rule` struct** to accept the same options. Add a `RuleOptions` type
   for rules that are configurable.
4. **Match the `*-errors.json`** output exactly — same messages, same spans.
5. **Match the `*-output.svelte`** exactly for fixable rules.
6. **Ignore `"only": true`** — that's a test runner flag, not rule config.
7. **Translate `settings.svelte.*`** into whatever `SvelteSettings` struct oxvelte uses.
   You'll need to design this struct once and make it available through `LintContext`.

### Coding conventions

- Use `oxc::allocator::Allocator` for all AST node allocation.
- All AST nodes carry `oxc::span::Span`.
- Parser errors use `oxc_diagnostics::OxcDiagnostic`.
- Lint rules implement `trait Rule { fn run(...) }` (see oxc_linter for reference pattern).
- Prefer `&str` over `String`; arena-allocate strings when they live in the AST.
- No `unsafe` unless strictly necessary and documented.
- `#[derive(Debug, Clone)]` on all AST nodes.
- Integration tests use `insta` for snapshot testing.
- Import oxc tools through the umbrella: `use oxc::{parser, semantic, allocator, span, ast}`.

---

## Svelte 5 Considerations

Svelte 5 introduces runes (`$state`, `$derived`, `$effect`, `$props`, `$bindable`,
`$inspect`, `$host`) and snippets (`{#snippet}` / `{@render}`). The parser must handle
both Svelte 4 legacy syntax and Svelte 5 modern syntax. The test fixtures in
`vendor/svelte` cover both.

Key Svelte 5 parsing differences:
- `<script module>` replaces `<script context="module">`
- Runes are parsed as regular JS by oxc (they look like function calls)
- `{#snippet name(params)}...{/snippet}` is a new block type
- `{@render snippet(args)}` is a new tag type
- `$props()` destructuring with `$bindable()` defaults
- `<svelte:boundary>` is a new special element

---

*This file is the human's interface. Edit it to steer the agent's priorities,
add new rules, adjust architecture decisions, or change the experimentation loop.*
