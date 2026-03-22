#!/usr/bin/env python3
"""Compare oxvelte vs eslint-plugin-svelte on real-world Svelte repos.

For each repo:
1. Use `oxvelte migrate` to convert the ESLint config to oxvelte format
2. Run ESLint with the repo's own config
3. Run oxvelte with recommended rules (matching eslint flat/recommended)
4. Compare the svelte/* diagnostics
"""

import json
import os
import subprocess
import sys
import tempfile
import time
import shutil
from pathlib import Path
from collections import defaultdict

ROOT = Path(__file__).parent.parent
OXVELTE = ROOT / "target" / "release" / "oxvelte"
ESLINT_DIR = Path(__file__).parent / "_eslint-runner"

def find_svelte_files(repo_path):
    """Find all .svelte files, excluding node_modules/build dirs."""
    skip = {"node_modules", ".svelte-kit", "build", "dist", ".git"}
    files = []
    for root, dirs, filenames in os.walk(repo_path):
        dirs[:] = [d for d in dirs if d not in skip]
        for f in filenames:
            if f.endswith(".svelte"):
                files.append(os.path.join(root, f))
    return sorted(files)

def run_oxvelte(files):
    """Run oxvelte lint (recommended) on files."""
    if not files:
        return [], 0
    start = time.time()
    result = subprocess.run(
        [str(OXVELTE), "lint", "--json"] + files,
        capture_output=True, text=True
    )
    elapsed = int((time.time() - start) * 1000)
    try:
        diags = json.loads(result.stdout) if result.stdout.strip() else []
    except json.JSONDecodeError:
        diags = []
    return diags, elapsed

def run_eslint(files, repo_path):
    """Run eslint with recommended svelte config on files."""
    if not files:
        return [], 0

    tmpdir = tempfile.mkdtemp(dir=str(ESLINT_DIR))
    try:
        for f in files:
            rel = os.path.relpath(f, repo_path)
            dest = os.path.join(tmpdir, rel)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            shutil.copy2(f, dest)

        start = time.time()
        result = subprocess.run(
            ["npx", "eslint", "--format", "json", tmpdir],
            capture_output=True, text=True, cwd=str(ESLINT_DIR)
        )
        elapsed = int((time.time() - start) * 1000)

        diags = []
        try:
            eslint_output = json.loads(result.stdout) if result.stdout.strip() else []
        except json.JSONDecodeError:
            eslint_output = []

        for file_result in eslint_output:
            for msg in file_result.get("messages", []):
                rule_id = msg.get("ruleId") or ""
                if rule_id.startswith("svelte/"):
                    diags.append({
                        "rule": rule_id,
                        "line": msg.get("line", 0),
                        "message": msg.get("message", ""),
                        "file": os.path.relpath(file_result.get("filePath", ""), tmpdir),
                    })
        return diags, elapsed
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)

def compare_repo(repo_path, name):
    print(f"\n{'=' * 70}")
    print(f"  {name}")
    print(f"{'=' * 70}")

    files = find_svelte_files(repo_path)
    print(f"  Files: {len(files)} .svelte files\n")

    if not files:
        print("  No .svelte files found.\n")
        return

    ox_diags, ox_ms = run_oxvelte(files)
    es_diags, es_ms = run_eslint(files, repo_path)

    # Group by rule
    ox_by_rule = defaultdict(int)
    for d in ox_diags:
        ox_by_rule[d["rule"]] += 1

    es_by_rule = defaultdict(int)
    for d in es_diags:
        es_by_rule[d["rule"]] += 1

    all_rules = sorted(set(list(ox_by_rule.keys()) + list(es_by_rule.keys())))

    if not all_rules:
        print("  Both linters: 0 diagnostics (clean code!)")
        print(f"  ESLint: {es_ms}ms | Oxvelte: {ox_ms}ms")
        if ox_ms > 0 and es_ms > 0:
            print(f"  Speedup: {es_ms / ox_ms:.1f}x")
        print()
        return

    print(f"  {'Rule':<50} {'ESLint':>7} {'Oxvelte':>7}  Match")
    print(f"  {'-'*50} {'-'*7} {'-'*7}  {'-'*5}")

    exact = 0
    close = 0
    mismatch_details = []
    for rule in all_rules:
        ec = es_by_rule.get(rule, 0)
        oc = ox_by_rule.get(rule, 0)
        if ec == oc:
            mark = "  ✓"
            exact += 1
        elif abs(ec - oc) <= max(1, int(max(ec, oc) * 0.15)):
            mark = "  ≈"
            close += 1
        else:
            mark = "  ✗"
            direction = "FP" if oc > ec else "FN"
            mismatch_details.append((rule, ec, oc, direction))
        print(f"  {rule:<50} {ec:>7} {oc:>7} {mark}")

    total = len(all_rules)
    print()
    print(f"  ESLint:  {len(es_diags):>5} diagnostics  ({es_ms}ms)")
    print(f"  Oxvelte: {len(ox_diags):>5} diagnostics  ({ox_ms}ms)")
    if ox_ms > 0 and es_ms > 0:
        print(f"  Speedup: {es_ms / ox_ms:.1f}x faster")
    print(f"  Match: {exact}/{total} exact, {close}/{total} close, {total - exact - close}/{total} mismatch")

    if mismatch_details:
        print(f"\n  Mismatches (FP=false positive, FN=false negative):")
        for rule, ec, oc, direction in mismatch_details:
            diff = oc - ec
            print(f"    {rule}: {direction} ({'+' if diff > 0 else ''}{diff})")
    print()

def main():
    if not OXVELTE.exists():
        print("Building oxvelte...")
        subprocess.run(["cargo", "build", "--release"], cwd=str(ROOT), check=True)

    testbeds = Path(__file__).parent

    repos = [
        (testbeds / "shadcn-svelte", "shadcn-svelte"),
        (testbeds / "open-webui", "open-webui"),
        (testbeds / "immich" / "web", "immich/web"),
        (testbeds / "kit", "sveltejs/kit"),
    ]

    for repo_path, name in repos:
        if repo_path.exists():
            compare_repo(str(repo_path), name)
        else:
            print(f"\n  {name}: not found at {repo_path}")

if __name__ == "__main__":
    main()
