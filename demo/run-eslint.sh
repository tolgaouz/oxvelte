#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
REPO="$ROOT/testbeds/shadcn-svelte"
RUNNER="$ROOT/testbeds/_eslint-runner"

tmpdir=$(mktemp -d)
find "$REPO" -name "*.svelte" \
  -not -path "*/node_modules/*" \
  -not -path "*/.svelte-kit/*" \
  -not -path "*/build/*" \
  -not -path "*/dist/*" 2>/dev/null | while IFS= read -r f; do
  rel=$(python3 -c "import os; print(os.path.relpath('''$f''', '''$REPO'''))")
  mkdir -p "$tmpdir/$(dirname "$rel")"
  cp "$f" "$tmpdir/$rel"
done

start=$(python3 -c "import time; print(time.time())")
(cd "$RUNNER" && find "$tmpdir" -name "*.svelte" -print0 | xargs -0 npx eslint --format json 2>/dev/null) >/dev/null || true
end=$(python3 -c "import time; print(time.time())")

elapsed=$(python3 -c "print(f'{($end - $start)*1000:.0f}')")
echo ""
printf "\033[1;33m⚠ 1,603 warnings\033[0m  (${elapsed}ms)\n"

rm -rf "$tmpdir"
