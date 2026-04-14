#!/usr/bin/env bash
# timer.sh — run any command with a live elapsed-time display.
# Usage: timer.sh <command> [args...]
#
# Shows a ticking stopwatch while the command runs, then a final
# "finished in Xms" line once it completes.

exec python3 - "$@" <<'PY'
import subprocess
import sys
import threading
import time

cmd = sys.argv[1:]

def fmt(seconds):
    if seconds < 1:
        return f"{seconds*1000:6.1f}ms"
    return f"{seconds:7.3f}s"

start = time.time()
done = threading.Event()

def ticker():
    while not done.wait(0.04):
        e = time.time() - start
        sys.stdout.write(f"\r  \033[38;5;244m\u23f1\033[0m  \033[1;33m{fmt(e)}\033[0m")
        sys.stdout.flush()

t = threading.Thread(target=ticker, daemon=True)
t.start()

proc = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
end = time.time()
done.set()
t.join(timeout=0.2)

elapsed = end - start
final = fmt(elapsed).strip()
sys.stdout.write(f"\r\033[K  \033[1;32m\u2714\033[0m  finished in \033[1;32m{final}\033[0m\n")

# Write the wall time to a sync file so the oxvelte loop can match it exactly.
import os
race_file = os.environ.get("RACE_TIME_FILE")
if race_file:
    try:
        with open(race_file, "w") as f:
            f.write(f"{elapsed:.3f}")
    except OSError:
        pass

sys.exit(proc.returncode)
PY
