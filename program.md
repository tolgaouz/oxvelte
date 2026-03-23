# Oxvelte — Performance Program

> Minimize p95 lint time on real-world Svelte codebases. The agent profiles,
> optimizes, and iterates until oxvelte is as fast as possible without
> sacrificing correctness.

---

## Goal

**Minimize wall-clock time** for `oxvelte lint` on four real-world repos.
The primary metric is **p95 latency** (95th-percentile of 20 runs).

### Baseline (2026-03-22)

| Repo | Files | p95 (ms) | ms/file |
|------|-------|----------|---------|
| shadcn-svelte | 1603 | ~147 | 0.09 |
| open-webui | 549 | ~3689 | 6.72 |
| immich/web | 400 | ~510 | 1.28 |
| sveltejs/kit | 876 | ~71 | 0.08 |

open-webui is the bottleneck — 50x slower per-file than kit. Focus there first.

### Correctness Constraint

All optimizations must preserve correctness:

- `cargo test --lib` must pass 281/281
- `bash verify_fixtures.sh --verify` must pass
- `python3 testbeds/compare.py` must show no regressions vs current parity

---

## Testbed

Repos are cloned in `testbeds/`:

```
testbeds/
  shadcn-svelte/   ← 1603 .svelte files
  open-webui/      ← 549 .svelte files
  immich/          ← 400 .svelte files
  kit/             ← 876 .svelte files
  results.tsv      ← timing results (append-only log)
```

---

## Benchmarking

### How to measure

```bash
# Single run with timing (ms):
python3 -c "
import subprocess, time
repos = [
    ('shadcn-svelte', 'testbeds/shadcn-svelte'),
    ('open-webui', 'testbeds/open-webui'),
    ('immich', 'testbeds/immich'),
    ('kit', 'testbeds/kit'),
]
for name, path in repos:
    times = []
    for _ in range(20):
        start = time.time()
        subprocess.run(['./target/release/oxvelte', 'lint', '--json', path],
                       capture_output=True)
        times.append(int((time.time() - start) * 1000))
    times.sort()
    p50 = times[9]
    p95 = times[18]
    print(f'{name}: p50={p50}ms p95={p95}ms min={times[0]}ms max={times[-1]}ms')
"
```

### results.tsv format

Append a row after every successful optimization:

```
timestamp	commit	shadcn_p95	openwebui_p95	immich_p95	kit_p95	notes
```

Create the file with headers if it doesn't exist, then append data rows.

---

## Profiling Techniques

### CPU profiling (samply / cargo-flamegraph)

```bash
# Generate flamegraph
cargo flamegraph --release -- lint testbeds/open-webui/ --json > /dev/null

# Or use samply
samply record ./target/release/oxvelte lint testbeds/open-webui/ --json > /dev/null
```

### Quick hotspot identification

```bash
# Time individual phases by adding --timing flag (if available)
# Or instrument with std::time::Instant in code

# Check which files are slow
for f in testbeds/open-webui/src/lib/components/**/*.svelte; do
    t=$(python3 -c "import subprocess,time; s=time.time(); subprocess.run(['./target/release/oxvelte','lint','--json','$f'],capture_output=True); print(int((time.time()-s)*1000))")
    [ "$t" -gt 50 ] && echo "${t}ms $f"
done
```

### Memory profiling

```bash
# Check peak RSS
/usr/bin/time -l ./target/release/oxvelte lint testbeds/open-webui/ --json > /dev/null
```

---

## Optimization Strategies (ordered by likely impact)

### 1. Parallelism
- **Multi-threaded file processing**: Use rayon to parse + lint files in parallel.
  Currently all files are processed sequentially.
- Thread-pool with work-stealing for uneven file sizes.

### 2. Parser hot paths
- Profile the parser to find slow regex/string operations.
- Avoid unnecessary string allocations (use `&str` slices instead of `String`).
- Pre-compile patterns used in hot loops.

### 3. Rule-level optimization
- Rules that scan the full file text multiple times → merge into single pass.
- `infinite-reactive-loop` does many nested loops with string searches → optimize.
- `no-reactive-reassign` has O(vars × content) pattern matching → batch.
- Pre-compute shared data (e.g., variable declarations) once, share across rules.

### 4. I/O
- Batch file reads with memory-mapped I/O or read-ahead.
- Avoid redundant file system operations.

### 5. Allocations
- Reduce `String` allocations in hot paths (use `Cow<str>` or arena allocation).
- Profile with `dhat` or `heaptrack` to find allocation hotspots.
- Reuse buffers across files.

---

## Experiment Loop

```
while improvement_possible:
    1. Profile:  cargo flamegraph / samply on the slowest repo
    2. Identify the hotspot (function taking most time)
    3. Implement optimization in src/
    4. Build:  cargo build --release
    5. Verify correctness:
       - cargo test --lib (281/281)
       - bash verify_fixtures.sh --verify
    6. Benchmark:  run 20 iterations per repo
    7. Record in results.tsv
    8. Commit if improved:
         git add src/ && git commit -m "perf: <description> — <repo> p95 Xms→Yms"
       If regressed or broke correctness: revert
    9. Loop
```

---

## Constraints

- **Only modify files in `src/`** — testbeds and fixtures are read-only
- **Build must pass**: `cargo build --release` before every commit
- **Tests must not regress**: `cargo test --lib` must pass 281/281
- **Fixture integrity**: `bash verify_fixtures.sh --verify` before every commit
- **Correctness**: `python3 testbeds/compare.py` must show no regressions
- **Timeout**: no single process > 2 minutes
- **Never stop**: run the loop autonomously until diminishing returns

---

## Targets

| Repo | Baseline p95 | Target p95 | Stretch |
|------|-------------|------------|---------|
| shadcn-svelte | 147ms | <100ms | <50ms |
| open-webui | 3689ms | <500ms | <200ms |
| immich | 510ms | <200ms | <100ms |
| kit | 71ms | <50ms | <30ms |

The primary target is **open-webui < 500ms** (7x improvement).
