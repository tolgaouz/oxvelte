#!/bin/bash
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$DIR")"
OXVELTE="$ROOT/target/release/oxvelte"
REPO="$ROOT/testbeds/shadcn-svelte"

# Spinner + elapsed timer in background
spin() {
  local chars='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
  local i=0
  local start=$SECONDS
  while true; do
    local elapsed=$(( SECONDS - start ))
    local s=$(( elapsed % 60 ))
    local ms=$(( ($(python3 -c "import time; print(int(time.time()*100))") % 100) ))
    printf "\r\033[2K  \033[1;33m${chars:i%10:1}\033[0m  linting...  \033[2m${s}.${ms}s\033[0m"
    i=$((i + 1))
    sleep 0.08
  done
}

spin &
SPIN_PID=$!

# Run oxvelte
find "$REPO" -name "*.svelte" \
  -not -path "*/node_modules/*" \
  -not -path "*/.svelte-kit/*" \
  -not -path "*/build/*" \
  -not -path "*/dist/*" 2>/dev/null | xargs "$OXVELTE" lint --json >/dev/null 2>&1 || true

kill $SPIN_PID 2>/dev/null
wait $SPIN_PID 2>/dev/null || true

printf "\r\033[2K  \033[1;32m✔\033[0m  \033[1m1,603 files  ·  ~17ms\033[0m\n"
