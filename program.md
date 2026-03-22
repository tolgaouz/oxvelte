# Oxvelte — Parity Program

> Eliminate every discrepancy between oxvelte and eslint-plugin-svelte on real-world
> Svelte codebases. The agent iterates until both linters produce identical diagnostics.

---

## Goal

**Zero false positives, zero false negatives** when comparing oxvelte's recommended
rules against eslint-plugin-svelte's `flat/recommended` config on real-world repos.

### Primary Benchmark: Real-World Comparison

Four public repos serve as the ground truth:

| Repo | Files | Baseline ESLint | Baseline Oxvelte | Gap |
|------|-------|-----------------|------------------|-----|
| shadcn-svelte (1603 files) | 0 diags | 0 diags | ✓ match |
| open-webui (549 files) | 94 diags | 0 diags | 94 FN |
| immich/web (400 files) | 0 diags | 109 diags | 109 FP |
| sveltejs/kit (876 files) | 17 diags | 345 diags | 328 FP, 1 match |

**Total gap: 531 diagnostics** (203 false negatives + 328 false positives).
Target: 0.

### Secondary Benchmark: Fixture Tests

All vendor fixture tests from eslint-plugin-svelte must continue passing:

- **280 test functions** covering 1156 fixture files
- **106 parser fixture tests** (83 legacy + 23 modern)
- No regressions allowed

---

## Testbed Setup

Repos are cloned in `testbeds/`:

```
testbeds/
  shadcn-svelte/          ← git clone --depth 1 https://github.com/huntabyte/shadcn-svelte
  open-webui/             ← git clone --depth 1 https://github.com/open-webui/open-webui
  immich/                 ← git clone --depth 1 --sparse (web/) https://github.com/immich-app/immich
  kit/                    ← git clone --depth 1 https://github.com/sveltejs/kit
  _eslint-runner/         ← standalone eslint + eslint-plugin-svelte (flat/recommended)
  compare.py              ← comparison script
  comparison-results.txt  ← latest results
```

The `_eslint-runner/` directory has a standalone ESLint installation with
`eslint-plugin-svelte`'s `flat/recommended` config. It lints files by copying
them to a temp dir, running `npx eslint --format json`, and extracting
`svelte/*` diagnostics.

---

## Current Discrepancies (Baseline)

### False Positives (oxvelte fires, ESLint doesn't) — 437 total

| Rule | Count | Repos | Root Cause |
|------|-------|-------|------------|
| `no-navigation-without-resolve` | 309 | kit(298), immich(11) | Flags `<a href>` in non-SvelteKit route files; too aggressive on test fixtures |
| `no-unused-props` | 64 | immich(64) | Likely false-flags Svelte 5 `$props()` patterns or imported types |
| `valid-each-key` | 17 | immich(17) | Flags valid each blocks incorrectly |
| `valid-prop-names-in-kit-pages` | 9 | kit(9) | Flags non-page files as SvelteKit pages |
| `no-dom-manipulating` | 5 | immich(5) | Flags legitimate DOM access patterns |
| `no-inner-declarations` | 4 | immich(4) | Flags declarations that are actually valid |
| `no-unknown-style-directive-property` | 4 | immich(4) | Flags valid CSS properties |
| `no-useless-mustaches` | 4 | kit(4) | Flags mustaches that aren't actually useless |
| `require-store-reactive-access` | 4 | immich(4) | Flags non-store imports as stores |
| `require-event-dispatcher-types` | 4 | kit(4) | Flags components that don't use dispatchers |
| `require-each-key` | 7 | kit(7) | Flags each blocks that have keys or don't need them |
| `no-immutable-reactive-statements` | 3 | kit(3) | False flags on mutable reactive statements |
| `system` | 3 | kit(3) | System rule shouldn't produce diagnostics |

### False Negatives (ESLint fires, oxvelte misses) — 94 total

| Rule | Count | Repos | Root Cause |
|------|-------|-------|------------|
| `require-each-key` | 56 | open-webui(56) | Missing `{#each}` patterns (possibly inside control flow) |
| `no-useless-mustaches` | 23 | open-webui(23) | Missing expression patterns |
| `infinite-reactive-loop` | 8 | open-webui(8) | Missing async/reactive patterns |
| `no-at-html-tags` | 4 | open-webui(4) | Missing `{@html}` in certain template contexts |
| `no-unused-svelte-ignore` | 2 | open-webui(2) | Rule not implemented (needs Svelte compiler) — skip |

---

## Experiment Loop

```
while gap > 0:
    1. Run comparison:  python3 testbeds/compare.py
    2. Pick the highest-impact discrepancy (most diagnostics off)
    3. Investigate: sample 3-5 specific files causing the mismatch
       - For FP: read the file, understand why ESLint doesn't flag it
       - For FN: read the file, understand why ESLint DOES flag it
    4. Fix the rule implementation in src/linter/rules/
    5. Build:  cargo build --release
    6. Verify fixture tests:  cargo test --lib
       - Must still pass 280/280. If any regress, fix or revert.
    7. Re-run comparison:  python3 testbeds/compare.py
       - Record new gap numbers
    8. Commit if improved:
         git add src/ && git commit -m "parity: <rule> — <FP|FN> reduced by N"
       If regressed: revert
    9. Loop
```

---

## Investigation Techniques

### For False Positives

```bash
# Find what oxvelte flags that ESLint doesn't
./target/release/oxvelte lint --json testbeds/<repo> 2>/dev/null | \
  python3 -c "import json,sys; [print(f\"{d['file']}:{d['line']} [{d['rule']}] {d['message']}\") for d in json.load(sys.stdin) if d['rule']=='svelte/<rule-name>']" | head -10
```

Then read the specific files and understand:
- Is the code actually valid? (ESLint is right, oxvelte is wrong)
- Does the file use a pattern the rule doesn't handle?
- Is there a config option that suppresses this in ESLint but not oxvelte?

### For False Negatives

```bash
# Find what ESLint flags that oxvelte doesn't — run ESLint on specific files
cd testbeds/_eslint-runner
npx eslint --format json <file> | python3 -c "..."
```

Then check:
- Does oxvelte's parser correctly parse the template? (parser bug?)
- Does the rule's pattern matching miss this code shape?
- Is there an AST node type the rule doesn't visit?

### Vendor Reference

Always check the eslint-plugin-svelte source for the rule's actual logic:
```
vendor/eslint-plugin-svelte/packages/eslint-plugin-svelte/src/rules/<rule-name>.ts
```

---

## Priority Order

Fix in this order (highest impact first):

1. **`no-navigation-without-resolve`** — 309 FP (restrict to SvelteKit route files)
2. **`no-unused-props`** — 64 FP (Svelte 5 $props() patterns)
3. **`require-each-key`** — 56 FN + 7 FP (missing patterns + false flags)
4. **`no-useless-mustaches`** — 23 FN + 4 FP
5. **`valid-each-key`** — 17 FP
6. **`valid-prop-names-in-kit-pages`** — 9 FP
7. **`infinite-reactive-loop`** — 8 FN
8. **`no-dom-manipulating`** — 5 FP
9. **`no-at-html-tags`** — 4 FN
10. **Remaining small FP/FN** (≤4 each)

---

## Rules to Skip

- `no-unused-svelte-ignore` — requires Svelte compiler to know which ignores are used
- `valid-compile` — IS the Svelte compiler
- `system` — internal eslint-plugin-svelte rule, should produce 0 diagnostics

---

## Constraints

Same as the original program:

- **Only modify files in `src/`** — everything else is read-only
- **Build must pass**: `cargo build` before every commit
- **Fixture tests must not regress**: `cargo test --lib` must pass 280/280
- **Fixture integrity**: `bash verify_fixtures.sh --verify` before every commit
- **Timeout**: no process > 2 minutes
- **Never stop**: run the loop autonomously until gap reaches 0

---

## Metrics

After every experiment, record in `testbeds/comparison-results.txt`:

```
Run comparison:  python3 testbeds/compare.py > testbeds/comparison-results.txt
```

Track the total gap: `sum(|ESLint_count - Oxvelte_count|)` across all rules and repos.

Baseline gap: **531**. Target: **0**.
