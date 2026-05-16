#!/usr/bin/env bash
# Compare oxvelte against a testbed's own eslint: its config, installed
# eslint-plugin-svelte, and pinned Svelte version. No synthetic runner.
#
# Usage:
#   ./scripts/parity-real.sh <testbed>
#   ./scripts/parity-real.sh <testbed> --skip-install   # node_modules present
#
# Testbed metadata lives in scripts/testbeds.tsv.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST_FILE="$ROOT/scripts/testbeds.tsv"
SKIPPED_MANIFEST_FILE="$ROOT/scripts/testbeds-skipped.tsv"

NAME="${1:-}"
[ -n "$NAME" ] || { echo "usage: $0 <testbed> [--skip-install]" >&2; exit 2; }
shift || true

SKIP_INSTALL=0
for a in "$@"; do
  case "$a" in
    --skip-install) SKIP_INSTALL=1 ;;
    --*) echo "unknown flag: $a" >&2; exit 2 ;;
  esac
done

TB="$ROOT/testbeds/$NAME"
[ -d "$TB" ] || { echo "no such testbed: $TB" >&2; exit 1; }
OXVELTE="$ROOT/target/release/oxvelte"
[ -x "$OXVELTE" ] || { echo "build oxvelte first: cargo build --release" >&2; exit 1; }

URL=""
PIN="-"
PACKAGE_ROOT_REL="."
ESLINT_ROOT_REL="."
while IFS=$'\t' read -r manifest_name url pin package_root eslint_root; do
  case "$manifest_name" in ''|\#*) continue ;; esac
  if [ "$manifest_name" = "$NAME" ]; then
    URL="$url"
    PIN="${pin:-}"
    PACKAGE_ROOT_REL="${package_root:-.}"
    ESLINT_ROOT_REL="${eslint_root:-.}"
    break
  fi
done < "$MANIFEST_FILE"
if [ -z "$URL" ]; then
  while IFS=$'\t' read -r skipped_name skipped_url skipped_pin skipped_package_root skipped_eslint_root skipped_status skipped_reason; do
    case "$skipped_name" in ''|\#*) continue ;; esac
    if [ "$skipped_name" = "$NAME" ]; then
      echo "testbed is skipped: $NAME ($skipped_status) $skipped_reason" >&2
      exit 1
    fi
  done < "$SKIPPED_MANIFEST_FILE"
  echo "unknown testbed in $MANIFEST_FILE: $NAME" >&2
  exit 1
fi

join_rel() {
  if [ -z "$2" ] || [ "$2" = "." ]; then
    printf '%s' "$1"
  else
    printf '%s/%s' "$1" "$2"
  fi
}

PACKAGE_ROOT="$(join_rel "$TB" "$PACKAGE_ROOT_REL")"
[ -d "$PACKAGE_ROOT" ] || { echo "package root does not exist: $PACKAGE_ROOT" >&2; exit 1; }

if [ -n "$ESLINT_ROOT_REL" ] && [ "$ESLINT_ROOT_REL" != "." ]; then
  ESLINT_ROOT="$(join_rel "$TB" "$ESLINT_ROOT_REL")"
  [ -d "$ESLINT_ROOT" ] || { echo "eslint root does not exist: $ESLINT_ROOT" >&2; exit 1; }
else
  ESLINT_ROOT=""
fi

find_eslint_config() {
  find "$1" -maxdepth "$2" \
    \( -name 'eslint.config.js' -o -name 'eslint.config.mjs' -o -name 'eslint.config.cjs' -o -name 'eslint.config.ts' \
       -o -name '.eslintrc.js' -o -name '.eslintrc.cjs' -o -name '.eslintrc.json' -o -name '.eslintrc.yaml' \) \
    -not -path "*/.claude/*" \
    -not -path "*/.git/*" \
    -not -path "*/.worktrees/*" \
    -not -path "*/node_modules/*" 2>/dev/null | sort | head -1
}

if [ -n "$ESLINT_ROOT" ]; then
  ESLINT_CFG="$(find_eslint_config "$ESLINT_ROOT" 1)"
else
  ESLINT_CFG="$(find_eslint_config "$PACKAGE_ROOT" 4)"
  if [ -z "$ESLINT_CFG" ] && [ "$PACKAGE_ROOT" != "$TB" ]; then
    ESLINT_CFG="$(find_eslint_config "$TB" 4)"
  fi
  [ -n "$ESLINT_CFG" ] || { echo "no eslint config in $NAME" >&2; exit 1; }
  ESLINT_ROOT="$(dirname "$ESLINT_CFG")"
fi
[ -n "$ESLINT_CFG" ] || { echo "no eslint config in $ESLINT_ROOT" >&2; exit 1; }

echo "==> $NAME  package: ${PACKAGE_ROOT#$ROOT/}  config: ${ESLINT_CFG#$ROOT/}"

# --- detect package manager ---------------------------------------------------
if   [ -f "$PACKAGE_ROOT/pnpm-lock.yaml"    ]; then PM=pnpm
elif [ -f "$PACKAGE_ROOT/yarn.lock"         ]; then PM=yarn
elif [ -f "$PACKAGE_ROOT/bun.lock"          ] || [ -f "$PACKAGE_ROOT/bun.lockb" ]; then PM=bun
elif [ -f "$PACKAGE_ROOT/package-lock.json" ]; then PM=npm
else echo "no lockfile in $PACKAGE_ROOT" >&2; exit 1
fi

# --- install if missing -------------------------------------------------------
if [ "$SKIP_INSTALL" = 0 ] && [ ! -d "$PACKAGE_ROOT/node_modules" ] && [ ! -d "$ESLINT_ROOT/node_modules" ]; then
  echo "==> installing deps with $PM (one-time)"
  case $PM in
    pnpm) ( cd "$PACKAGE_ROOT" && pnpm install --prefer-offline --ignore-scripts ) ;;
    yarn) ( cd "$PACKAGE_ROOT" && yarn install --ignore-scripts ) ;;
    bun)  ( cd "$PACKAGE_ROOT" && bun install --no-save ) ;;
    npm)  ( cd "$PACKAGE_ROOT" && npm install --no-audit --no-fund --ignore-scripts ) ;;
  esac
fi

# --- collect svelte files (bash 3.2 compatible - no mapfile) ------------------
FILES=()
while IFS= read -r f; do FILES+=("$f"); done < <(find "$TB" -name '*.svelte' \
  -not -path "*/node_modules/*" \
  -not -path "*/.claude/*" \
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
ES_CFG_JSON="$OUT_DIR/$NAME-eslint-config.json"
OX_CFG_JSON="$OUT_DIR/$NAME-oxvelte.config.json"
METADATA_JSON="$OUT_DIR/$NAME-metadata.json"

classify_eslint_failure() {
  local stderr_file="$1"
  local output_file="${2:-}"
  if [ -f "$stderr_file" ] && grep -Eiq 'heap out of memory|Allocation failed - JavaScript heap out of memory|Ineffective mark-compacts|Abort trap' "$stderr_file"; then
    printf 'eslint-oom\tESLint exhausted the Node heap while producing the parity baseline.\n'
    return 0
  fi
  if [ -f "$stderr_file" ] && grep -Fq 'scopeManager.addGlobals is not a function' "$stderr_file"; then
    printf 'eslint-runtime-crash\tESLint crashed before producing a usable JSON baseline.\n'
    return 0
  fi
  if [ -f "$stderr_file" ] && grep -Eiq 'serialize configuration data|preprocess\.markup' "$stderr_file"; then
    printf 'eslint-print-config-unserializable\tESLint --print-config cannot serialize the project config.\n'
    return 0
  fi
  if [ -f "$stderr_file" ] && grep -Eiq 'Configuration for rule .* is invalid|Severity should be one of|no-console.*log' "$stderr_file"; then
    printf 'invalid-eslint-config\tThe project ESLint config is invalid for this ESLint run.\n'
    return 0
  fi
  if [ -n "$output_file" ] && [ -s "$output_file" ] && grep -qx 'undefined' "$output_file"; then
    printf 'eslint-print-config-invalid\tESLint --print-config returned undefined instead of JSON.\n'
    return 0
  fi
  printf 'eslint-setup-failure\tESLint did not produce a usable parity baseline.\n'
}

write_setup_failure_metadata() {
  local stage="$1"
  local status="$2"
  local category="$3"
  local reason="$4"
  local output_file="${5:-}"
  local stderr_file="${6:-}"
  python3 - "$METADATA_JSON" "$NAME" "$URL" "$TB" "$PACKAGE_ROOT" "$ESLINT_ROOT" "$stage" "$status" "$category" "$reason" "$output_file" "$stderr_file" <<'PYEOF'
import json
import os
import sys

(
    metadata_path,
    name,
    url,
    tb,
    package_root,
    eslint_root,
    stage,
    status,
    category,
    reason,
    output_file,
    stderr_file,
) = sys.argv[1:]

def rel(path):
    return os.path.relpath(path, tb) if path else ""

def tail(path, limit=4000):
    if not path or not os.path.exists(path):
        return ""
    with open(path, errors="replace") as f:
        text = f.read()
    return text[-limit:]

metadata = {
    "name": name,
    "url": url,
    "status": "setup-failed",
    "stage": stage,
    "category": category,
    "reason": reason,
    "eslintStatus": int(status) if str(status).lstrip("-").isdigit() else status,
    "eslintRoot": rel(eslint_root),
    "packageRoot": rel(package_root),
    "outputBytes": os.path.getsize(output_file) if output_file and os.path.exists(output_file) else 0,
    "stderrTail": tail(stderr_file),
}
with open(metadata_path, "w") as f:
    json.dump(metadata, f, indent=2)
    f.write("\n")
PYEOF
  echo "  wrote $METADATA_JSON"
}

# --- run testbed's own eslint -------------------------------------------------
ESLINT_BIN=""
for candidate in \
  "$ESLINT_ROOT/node_modules/.bin/eslint" \
  "$PACKAGE_ROOT/node_modules/.bin/eslint" \
  "$TB/node_modules/.bin/eslint"
do
  if [ -x "$candidate" ]; then
    ESLINT_BIN="$candidate"
    break
  fi
done
if [ ! -x "$ESLINT_BIN" ]; then
  msg="no eslint binary under $PACKAGE_ROOT/node_modules/.bin or $ESLINT_ROOT/node_modules/.bin"
  echo "$msg" >&2
  write_setup_failure_metadata "eslint-binary" 127 "no-eslint-binary" "$msg" "" ""
  exit 1
fi
echo "==> eslint: $ESLINT_BIN"

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

PRINT_CONFIG_FILE="${FILES[0]}"
for f in "${FILES[@]}"; do
  case "$f" in
    "$ESLINT_ROOT"/*) PRINT_CONFIG_FILE="$f"; break ;;
  esac
done
echo "==> print-config: ${PRINT_CONFIG_FILE#$ROOT/}"

set +e
( cd "$ESLINT_ROOT" && "$ESLINT_BIN" "${RULE_ARGS[@]}" --print-config "$PRINT_CONFIG_FILE" \
) > "$ES_CFG_JSON" 2>"$OUT_DIR/$NAME-eslint.stderr"
print_config_status=$?
set -e
if [ "$print_config_status" -ne 0 ]; then
  failure="$(classify_eslint_failure "$OUT_DIR/$NAME-eslint.stderr" "$ES_CFG_JSON")"
  category="${failure%%$'\t'*}"
  reason="${failure#*$'\t'}"
  echo "eslint setup failed during --print-config: $category" >&2
  write_setup_failure_metadata "eslint-print-config" "$print_config_status" "$category" "$reason" "$ES_CFG_JSON" "$OUT_DIR/$NAME-eslint.stderr"
  exit 1
fi

if ! python3 - "$ES_CFG_JSON" "$OX_CFG_JSON" <<'PYEOF'
import json
import sys

eslint_config_path, oxvelte_config_path = sys.argv[1:]
with open(eslint_config_path) as f:
    config = json.load(f)

rules = {
    name: value
    for name, value in config.get("rules", {}).items()
    if name.startswith("svelte/")
}
out = {"rules": rules}
if "settings" in config:
    out["settings"] = config["settings"]

with open(oxvelte_config_path, "w") as f:
    json.dump(out, f, indent=2, sort_keys=True)
    f.write("\n")
PYEOF
then
  failure="$(classify_eslint_failure "$OUT_DIR/$NAME-eslint.stderr" "$ES_CFG_JSON")"
  category="${failure%%$'\t'*}"
  reason="${failure#*$'\t'}"
  echo "eslint setup failed: $category" >&2
  write_setup_failure_metadata "eslint-print-config-json" "$print_config_status" "$category" "$reason" "$ES_CFG_JSON" "$OUT_DIR/$NAME-eslint.stderr"
  exit 1
fi

echo "==> running testbed's eslint"
es_start=$(date +%s)
set +e
( cd "$ESLINT_ROOT" && NODE_OPTIONS="${NODE_OPTIONS:+$NODE_OPTIONS }--max-old-space-size=8192" \
    "$ESLINT_BIN" --no-error-on-unmatched-pattern --no-warn-ignored "${RULE_ARGS[@]}" --format json "${FILES[@]}" \
) > "$ES_JSON" 2>>"$OUT_DIR/$NAME-eslint.stderr"
eslint_status=$?
set -e
es_dur=$(( $(date +%s) - es_start ))

if ! python3 - "$ES_JSON" <<'PYEOF'
import json
import sys

path = sys.argv[1]
with open(path) as f:
    text = f.read().strip()
if not text:
    raise SystemExit("eslint produced empty JSON output")
value = json.loads(text)
if not isinstance(value, list):
    raise SystemExit("eslint JSON output is not a result list")
PYEOF
then
  failure="$(classify_eslint_failure "$OUT_DIR/$NAME-eslint.stderr" "$ES_JSON")"
  category="${failure%%$'\t'*}"
  reason="${failure#*$'\t'}"
  echo "eslint setup failed during lint: $category" >&2
  write_setup_failure_metadata "eslint-lint-json" "$eslint_status" "$category" "$reason" "$ES_JSON" "$OUT_DIR/$NAME-eslint.stderr"
  exit 1
fi
echo "   eslint: ${es_dur}s status=$eslint_status -> $(wc -c < "$ES_JSON") bytes"

# --- run oxvelte with the resolved ESLint-derived config ----------------------
echo "==> running oxvelte"
ox_start=$(date +%s)
set +e
"$OXVELTE" lint --all-rules --config "$OX_CFG_JSON" --json "${FILES[@]}" > "$OX_JSON" 2>"$OUT_DIR/$NAME-oxvelte.stderr"
oxvelte_status=$?
set -e
ox_dur=$(( $(date +%s) - ox_start ))

python3 - "$OX_JSON" <<'PYEOF'
import json
import sys

path = sys.argv[1]
with open(path) as f:
    text = f.read().strip()
if not text:
    raise SystemExit("oxvelte produced empty JSON output")
value = json.loads(text)
if not isinstance(value, list):
    raise SystemExit("oxvelte JSON output is not a diagnostic list")
PYEOF
echo "   oxvelte: ${ox_dur}s status=$oxvelte_status -> $(wc -c < "$OX_JSON") bytes"

# --- compare ------------------------------------------------------------------
COMMIT="$(git -C "$TB" rev-parse HEAD 2>/dev/null || printf unknown)"
SHALLOW="$(git -C "$TB" rev-parse --is-shallow-repository 2>/dev/null || printf unknown)"
ESLINT_VERSION="$("$ESLINT_BIN" --version 2>/dev/null || printf unknown)"
PLUGIN_VERSION="$(node - "$ESLINT_ROOT" "$PACKAGE_ROOT" <<'NODEEOF' 2>/dev/null || true
const roots = process.argv.slice(2);
for (const root of roots) {
  try {
    const pkg = require.resolve('eslint-plugin-svelte/package.json', { paths: [root] });
    console.log(require(pkg).version);
    process.exit(0);
  } catch {}
}
NODEEOF
)"
[ -n "$PLUGIN_VERSION" ] || PLUGIN_VERSION="unknown"

python3 - "$NAME" "$TB" "$ES_JSON" "$OX_JSON" "$OUT_DIR" "$OX_CFG_JSON" "$METADATA_JSON" "$URL" "$COMMIT" "$SHALLOW" "$ESLINT_ROOT" "$PACKAGE_ROOT" "$ESLINT_VERSION" "$PLUGIN_VERSION" "$eslint_status" "$oxvelte_status" <<'PYEOF'
import json
import os
import sys
from collections import defaultdict

(
    name,
    tb,
    es_path,
    ox_path,
    out_dir,
    ox_cfg_path,
    metadata_path,
    url,
    commit,
    shallow,
    eslint_root,
    package_root,
    eslint_version,
    plugin_version,
    eslint_status,
    oxvelte_status,
) = sys.argv[1:]

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

def load_json(path):
    with open(path) as f:
        return json.load(f)

def severity(value):
    if isinstance(value, list) and value:
        value = value[0]
    if isinstance(value, str):
        return value.lower()
    return value

def is_enabled(value):
    sev = severity(value)
    return sev not in (0, "0", "off", None)

es_raw = load_json(es_path)
ox_raw = load_json(ox_path)
ox_cfg = load_json(ox_cfg_path)
enabled_rules = {
    rule
    for rule, value in ox_cfg.get("rules", {}).items()
    if rule.startswith("svelte/") and is_enabled(value) and rule not in EXCLUDED
}

def rel(path):
    return os.path.relpath(path, tb)

# eslint output includes clean linted files; ignored files do not appear here.
in_scope = {rel(fr.get("filePath", "")) for fr in es_raw if fr.get("filePath")}

es = []
for fr in es_raw:
    fp = rel(fr.get("filePath", ""))
    for m in fr.get("messages", []):
        rid = m.get("ruleId") or ""
        if rid in enabled_rules:
            es.append((fp, rid, m.get("line", 0), m.get("message", "")))

ox = []
for d in ox_raw:
    rid = d.get("rule", "")
    fp = rel(d.get("file", ""))
    if fp in in_scope and rid in enabled_rules:
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
print(f"\n  totals - eslint={len(es_keys)}  oxvelte={len(ox_keys)}  match={len(match)}  fp={len(fps)}  fn={len(fns)}")

discrepancies_path = os.path.join(out_dir, f"{name}-discrepancies.json")
with open(discrepancies_path, "w") as f:
    json.dump({
        "fps": [{"file": k[0], "rule": k[1], "line": k[2], "message": ox_msgs.get(k, "")} for k in sorted(fps)],
        "fns": [{"file": k[0], "rule": k[1], "line": k[2], "message": es_msgs.get(k, "")} for k in sorted(fns)],
    }, f, indent=2)

metadata = {
    "name": name,
    "url": url,
    "commit": commit,
    "shallow": shallow,
    "eslintRoot": os.path.relpath(eslint_root, tb),
    "packageRoot": os.path.relpath(package_root, tb),
    "eslintVersion": eslint_version,
    "eslintPluginSvelteVersion": plugin_version,
    "eslintStatus": int(eslint_status),
    "oxvelteStatus": int(oxvelte_status),
    "enabledRules": sorted(enabled_rules),
    "excludedRules": sorted(EXCLUDED),
    "filesInScope": len(in_scope),
    "totals": {
        "eslint": len(es_keys),
        "oxvelte": len(ox_keys),
        "match": len(match),
        "fp": len(fps),
        "fn": len(fns),
    },
}
with open(metadata_path, "w") as f:
    json.dump(metadata, f, indent=2)
    f.write("\n")

print(f"  wrote {discrepancies_path}")
print(f"  wrote {metadata_path}")
PYEOF
