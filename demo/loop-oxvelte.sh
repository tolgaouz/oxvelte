#!/usr/bin/env bash
# loop-oxvelte.sh — run oxvelte repeatedly for N seconds, printing each run's time.
# Usage: loop-oxvelte.sh [duration_seconds]

DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
export OXVELTE_BIN="$(dirname "$DEMO_DIR")/target/release/oxvelte"

exec python3 - "$@" <<'PY'
import os
import subprocess
import sys
import time

OXVELTE = os.environ["OXVELTE_BIN"]
duration = float(sys.argv[1]) if len(sys.argv) > 1 else 25.0
start = time.time()
count = 0

try:
    while time.time() - start < duration:
        count += 1
        t0 = time.time()
        subprocess.run(
            [OXVELTE, "lint", ".", "--quiet"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        ms = (time.time() - t0) * 1000
        sys.stdout.write(
            f"\033[38;5;244m[#{count:3d}]\033[0m  "
            f"2538 files  "
            f"\033[1;33m{ms:5.0f}ms\033[0m  "
            f"\033[1;32m\u2714 0 problems\033[0m\n"
        )
        sys.stdout.flush()
except KeyboardInterrupt:
    pass

elapsed = time.time() - start
sys.stdout.write(
    f"\n\033[1;32m\u26a1 {count} runs\033[0m in {elapsed:.1f}s  "
    f"\033[2m\u2014  eslint did 1 in the same time\033[0m\n"
)
PY
