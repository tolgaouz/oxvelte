#!/usr/bin/env bash
# Compare oxvelte against a testbed's *own* eslint (its config, its installed
# eslint-plugin-svelte, its pinned Svelte version). No synthetic runner.
#
# Usage:
#   ./scripts/parity-real.sh <testbed>
#   ./scripts/parity-real.sh <testbed> --skip-install   # node_modules present
#
# Auto-detects package manager from lockfile and the eslint config root.

set -o pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NAME="${1:?usage: $0 <testbed> [--skip-install]}"; shift || true
SKIP_INSTALL=0
for a in "$@"; do [ "$a" = "--skip-install" ] && SKIP_INSTALL=1; done

TB="$ROOT/testbeds/$NAME"
[ -d "$TB" ] || { echo "no such testbed: $TB" >&2; exit 1; }
OXVELTE="$ROOT/target/release/oxvelte"
[ -x "$OXVELTE" ] || { echo "build oxvelte first: cargo build --release" >&2; exit 1; }

# --- detect package manager ---------------------------------------------------
if   [ -f "$TB/pnpm-lock.yaml"   ]; then PM=pnpm
elif [ -f "$TB/yarn.lock"        ]; then PM=yarn
elif [ -f "$TB/bun.lock"         ] || [ -f "$TB/bun.lockb" ]; then PM=bun
elif [ -f "$TB/package-lock.json" ]; then PM=npm
else echo "no lockfile in $TB" >&2; exit 1
fi

# --- detect eslint config root (the dir containing eslint.config.* / .eslintrc.*) ---
ESLINT_CFG=$(find "$TB" -maxdepth 4 \
  \( -name 'eslint.config.js' -o -name 'eslint.config.mjs' -o -name 'eslint.config.cjs' -o -name 'eslint.config.ts' \
     -o -name '.eslintrc.js' -o -name '.eslintrc.cjs' -o -name '.eslintrc.json' -o -name '.eslintrc.yaml' \) \
  -not -path "*/node_modules/*" 2>/dev/null | sort | head -1)
[ -n "$ESLINT_CFG" ] || { echo "no eslint config in $NAME" >&2; exit 1; }
ESLINT_ROOT="$(dirname "$ESLINT_CFG")"
echo "==> $NAME ($PM)  config: ${ESLINT_CFG#$ROOT/}"

# --- install if missing -------------------------------------------------------
if [ "$SKIP_INSTALL" = 0 ] && [ ! -d "$TB/node_modules" ]; then
  echo "==> installing deps with $PM (one-time)"
  case $PM in
    pnpm) ( cd "$TB" && pnpm install --prefer-offline --ignore-scripts ) ;;
    yarn) ( cd "$TB" && yarn install --ignore-scripts ) ;;
    bun)  ( cd "$TB" && bun install --no-save ) ;;
    npm)  ( cd "$TB" && npm install --no-audit --no-fund --ignore-scripts ) ;;
  esac
fi

# --- collect svelte files (bash 3.2 compatible — no `mapfile`) ----------------
FILES=()
while IFS= read -r f; do FILES+=("$f"); done < <(find "$TB" -name '*.svelte' \
  -not -path "*/node_modules/*" \
  -not -path "*/.svelte-kit/*" \
  -not -path "*/build/*" \
  -not -path "*/dist/*" \
  -not -path "*/.git/*" 2>/dev/null | sort)
N=${#FILES[@]}
echo "==> $N .svelte files"
[ "$N" -gt 0 ] || { echo "no svelte files" >&2; exit 0; }

OUT_DIR="$ROOT/reports/parity-real"
mkdir -p "$OUT_DIR"
ES_JSON="$OUT_DIR/$NAME-eslint.json"
OX_JSON="$OUT_DIR/$NAME-oxvelte.json"

# --- run testbed's own eslint -------------------------------------------------
# Resolve eslint binary inside the testbed's installed deps so we get the
# pinned plugin + parser + svelte versions.
ESLINT_BIN=$(find "$TB/node_modules/.bin/eslint" "$ESLINT_ROOT/node_modules/.bin/eslint" 2>/dev/null | head -1)
[ -x "$ESLINT_BIN" ] || { echo "no eslint binary under $TB/node_modules/.bin" >&2; exit 1; }
echo "==> eslint: $ESLINT_BIN"

# Rules we don't compare and that have known crashes in eslint-plugin-svelte 3.x
# (no-navigation-without-resolve crashes on certain SvelteKit syntax — disabling
#  it lets eslint produce output for the remaining files instead of bailing).
DISABLE_RULES=(
  'svelte/no-navigation-without-resolve'
  'svelte/no-navigation-without-base'
  'svelte/no-goto-without-base'
  'svelte/valid-prop-names-in-kit-pages'
  'svelte/no-export-load-in-svelte-module-in-kit-pages'
  'svelte/valid-compile'
  'svelte/no-unused-svelte-ignore'
  'svelte/no-unused-props'
  'svelte/require-store-reactive-access'
)
RULE_ARGS=()
for r in "${DISABLE_RULES[@]}"; do
  RULE_ARGS+=( --rule "{\"$r\": \"off\"}" )
done

echo "==> running testbed's eslint"
es_start=$(date +%s)
# Bump node heap — eslint with type-aware rules + thousands of svelte files
# (e.g. kit) blows past the 4 GB default.
( cd "$ESLINT_ROOT" && NODE_OPTIONS="${NODE_OPTIONS:-} --max-old-space-size=8192" \
    "$ESLINT_BIN" --no-error-on-unmatched-pattern --no-warn-ignored "${RULE_ARGS[@]}" --format json "${FILES[@]}" \
) > "$ES_JSON" 2>"$OUT_DIR/$NAME-eslint.stderr" || true
es_dur=$(( $(date +%s) - es_start ))
echo "   eslint: ${es_dur}s -> $(wc -c < "$ES_JSON") bytes"

# --- discover which svelte/* rules the testbed actually enables --------------
# Without this we'd flag rules disabled by config (e.g. require-each-key) as FPs.
# print-config returns the resolved config for a given file as JSON.
ES_CFG_JSON="$OUT_DIR/$NAME-eslint-config.json"
( cd "$ESLINT_ROOT" && "$ESLINT_BIN" --print-config "${FILES[0]}" \
) > "$ES_CFG_JSON" 2>>"$OUT_DIR/$NAME-eslint.stderr" || true

# --- run oxvelte --------------------------------------------------------------
echo "==> running oxvelte"
ox_start=$(date +%s)
"$OXVELTE" lint --json "${FILES[@]}" > "$OX_JSON" 2>"$OUT_DIR/$NAME-oxvelte.stderr" || true
ox_dur=$(( $(date +%s) - ox_start ))
echo "   oxvelte: ${ox_dur}s -> $(wc -c < "$OX_JSON") bytes"

# --- compare ------------------------------------------------------------------
python3 - "$NAME" "$TB" "$ES_JSON" "$OX_JSON" "$OUT_DIR" <<'PYEOF'
import json, os, sys
from collections import defaultdict

name, tb, es_path, ox_path, out_dir = sys.argv[1:]

EXCLUDED = {
    "svelte/no-navigation-without-resolve",
    "svelte/no-navigation-without-base",
    "svelte/no-goto-without-base",
    "svelte/valid-prop-names-in-kit-pages",
    "svelte/no-export-load-in-svelte-module-in-kit-pages",
    "svelte/valid-compile",
    "svelte/no-unused-svelte-ignore",
    "svelte/no-unused-props",
    "svelte/require-store-reactive-access",
}

def load(p):
    try:
        with open(p) as f:
            txt = f.read().strip()
        return json.loads(txt) if txt else []
    except (json.JSONDecodeError, FileNotFoundError):
        return []

es_raw = load(es_path)
ox_raw = load(ox_path)

# eslint output: list of {filePath, messages: [...]}
# Files that eslint's `ignores`/`.eslintignore` excludes don't appear here, so
# we use this set as the in-scope universe — comparing oxvelte's output on
# files eslint skipped would just measure the ignore-config divergence.
def rel(p): return os.path.relpath(p, tb)

in_scope = {rel(fr.get("filePath", "")) for fr in es_raw}

es = []
for fr in es_raw:
    fp = rel(fr.get("filePath", ""))
    for m in fr.get("messages", []):
        rid = m.get("ruleId") or ""
        if rid.startswith("svelte/") and rid not in EXCLUDED:
            es.append((fp, rid, m.get("line", 0), m.get("message", "")))

ox = []
for d in ox_raw:
    rid = d.get("rule", "")
    fp = rel(d.get("file", ""))
    if fp in in_scope and rid.startswith("svelte/") and rid not in EXCLUDED:
        ox.append((fp, rid, d.get("line", 0), d.get("message", "")))

es_keys = {(t[0], t[1], t[2]) for t in es}
ox_keys = {(t[0], t[1], t[2]) for t in ox}
match = es_keys & ox_keys
fps = ox_keys - es_keys
fns = es_keys - ox_keys

ox_msgs = {(t[0], t[1], t[2]): t[3] for t in ox}
es_msgs = {(t[0], t[1], t[2]): t[3] for t in es}

stats = defaultdict(lambda: {"es": 0, "ox": 0, "m": 0, "fp": 0, "fn": 0})
for k in es_keys: stats[k[1]]["es"] += 1
for k in ox_keys: stats[k[1]]["ox"] += 1
for k in match:   stats[k[1]]["m"]  += 1
for k in fps:     stats[k[1]]["fp"] += 1
for k in fns:     stats[k[1]]["fn"] += 1

print()
print(f"  {'rule':<48} {'eslint':>7} {'oxvelte':>7} {'match':>6} {'fp':>5} {'fn':>5}")
print(f"  {'-'*48} {'-'*7} {'-'*7} {'-'*6} {'-'*5} {'-'*5}")
for r in sorted(stats):
    s = stats[r]
    print(f"  {r:<48} {s['es']:>7} {s['ox']:>7} {s['m']:>6} {s['fp']:>5} {s['fn']:>5}")
print(f"\n  totals — eslint={len(es_keys)}  oxvelte={len(ox_keys)}  match={len(match)}  fp={len(fps)}  fn={len(fns)}")

# write per-testbed discrepancies file
out = os.path.join(out_dir, f"{name}-discrepancies.json")
with open(out, "w") as f:
    json.dump({
        "fps": [{"file": k[0], "rule": k[1], "line": k[2], "message": ox_msgs.get(k, "")} for k in sorted(fps)],
        "fns": [{"file": k[0], "rule": k[1], "line": k[2], "message": es_msgs.get(k, "")} for k in sorted(fns)],
    }, f, indent=2)
print(f"  wrote {out}")
PYEOF
