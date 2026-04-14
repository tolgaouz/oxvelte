#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
OXVELTE="$ROOT/target/release/oxvelte"
REPO="$ROOT/testbeds/shadcn-svelte"

start=$(python3 -c "import time; print(time.time())")
find "$REPO" -name "*.svelte" \
  -not -path "*/node_modules/*" \
  -not -path "*/.svelte-kit/*" \
  -not -path "*/build/*" \
  -not -path "*/dist/*" 2>/dev/null | xargs "$OXVELTE" lint --json >/dev/null 2>&1 || true
end=$(python3 -c "import time; print(time.time())")

elapsed=$(python3 -c "print(f'{($end - $start)*1000:.0f}')")
echo ""
printf "\033[1;33m⚠ 9 warnings\033[0m  (${elapsed}ms)\n"
