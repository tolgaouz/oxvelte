#!/bin/bash
# Compare oxvelte vs eslint-plugin-svelte on real-world repos
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
OXVELTE="$ROOT/target/release/oxvelte"
ESLINT_DIR="$SCRIPT_DIR/_eslint-runner"

if [ ! -f "$OXVELTE" ]; then
  echo "Building oxvelte..."
  cd "$ROOT" && cargo build --release 2>/dev/null
fi

compare_repo() {
  local repo_path="$1"
  local name="$2"

  echo "============================================================"
  echo "  $name"
  echo "============================================================"

  # Collect svelte files
  local svelte_files=$(find "$repo_path" -name "*.svelte" \
    -not -path "*/node_modules/*" \
    -not -path "*/.svelte-kit/*" \
    -not -path "*/build/*" \
    -not -path "*/dist/*" \
    2>/dev/null | sort)
  local file_count=$(echo "$svelte_files" | wc -l | tr -d ' ')
  echo "  Files: $file_count .svelte files"
  echo ""

  # Run oxvelte
  local ox_start=$(date +%s%N)
  local ox_json=$("$OXVELTE" lint --json $svelte_files 2>/dev/null || echo "[]")
  local ox_end=$(date +%s%N)
  local ox_ms=$(( (ox_end - ox_start) / 1000000 ))

  # Run eslint (copy files to runner dir temporarily)
  local tmpdir=$(mktemp -d)
  for f in $svelte_files; do
    local rel=$(python3 -c "import os; print(os.path.relpath('$f', '$repo_path'))")
    mkdir -p "$tmpdir/$(dirname "$rel")"
    cp "$f" "$tmpdir/$rel"
  done

  local es_start=$(date +%s%N)
  local es_json=$(cd "$ESLINT_DIR" && npx eslint --format json "$tmpdir"/**/*.svelte 2>/dev/null || echo "[]")
  local es_end=$(date +%s%N)
  local es_ms=$(( (es_end - es_start) / 1000000 ))
  rm -rf "$tmpdir"

  # Parse and compare
  python3 << PYEOF
import json, sys

ox = json.loads('''$ox_json''')
es_raw = json.loads('''$es_json''')

# Normalize eslint output
es = []
for f in es_raw:
    for m in f.get("messages", []):
        rid = m.get("ruleId") or ""
        if rid.startswith("svelte/"):
            es.append({"rule": rid, "message": m["message"]})

# Group by rule
ox_by_rule = {}
for d in ox:
    r = d["rule"]
    ox_by_rule[r] = ox_by_rule.get(r, 0) + 1

es_by_rule = {}
for d in es:
    r = d["rule"]
    es_by_rule[r] = es_by_rule.get(r, 0) + 1

all_rules = sorted(set(list(ox_by_rule.keys()) + list(es_by_rule.keys())))

print(f"  {'Rule':<50} {'ESLint':>8} {'Oxvelte':>8} {'Match':>6}")
print(f"  {'-'*50} {'-'*8} {'-'*8} {'-'*6}")

match_count = 0
total_rules = 0
for rule in all_rules:
    ec = es_by_rule.get(rule, 0)
    oc = ox_by_rule.get(rule, 0)
    match = "✓" if ec == oc else ("≈" if abs(ec - oc) <= 2 else "✗")
    if ec == oc:
        match_count += 1
    total_rules += 1
    print(f"  {rule:<50} {ec:>8} {oc:>8} {match:>6}")

print(f"")
print(f"  ESLint total: {len(es)} diagnostics ({es_ms}ms)")
print(f"  Oxvelte total: {len(ox)} diagnostics ({ox_ms}ms)")
if total_rules > 0:
    print(f"  Rule match rate: {match_count}/{total_rules} ({100*match_count//total_rules}%)")
print()
PYEOF
}

# Run comparisons
for repo in shadcn-svelte open-webui; do
  if [ -d "$SCRIPT_DIR/$repo" ]; then
    compare_repo "$SCRIPT_DIR/$repo" "$repo"
  fi
done

# immich web subdir
if [ -d "$SCRIPT_DIR/immich/web" ]; then
  compare_repo "$SCRIPT_DIR/immich/web" "immich/web"
fi

# kit - svelte files in packages
if [ -d "$SCRIPT_DIR/kit" ]; then
  compare_repo "$SCRIPT_DIR/kit" "sveltejs/kit"
fi
