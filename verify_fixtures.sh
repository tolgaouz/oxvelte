#!/usr/bin/env bash
set -euo pipefail

# verify_fixtures.sh — Verify that test fixtures have not been modified.
#
# Run this BEFORE committing. The experiment loop in program.md calls this
# as a gate: if fixtures have been tampered with, the commit is rejected.
#
# How it works:
#   1. On first run (after prepare.sh), generates .fixtures.sha256 from vendor/
#   2. On subsequent runs, re-checksums vendor/ copies and compares
#   3. Also verifies that fixtures/ copies match vendor/ originals exactly
#
# Do NOT modify this file.

CHECKSUM_FILE=".fixtures.sha256"
VENDOR_SVELTE="vendor/svelte/packages/svelte/tests"
VENDOR_ESLINT="vendor/eslint-plugin-svelte/packages/eslint-plugin-svelte/tests"
PARSER_FIXTURES="fixtures/parser"
LINTER_FIXTURES="fixtures/linter"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

# ─── Generate checksums from vendor (the source of truth) ──────────────────

generate_checksums() {
    echo "[verify] Generating fixture checksums from vendor/..."
    {
        # Checksum every test fixture file in vendor
        find "$VENDOR_SVELTE" -type f \( -name "*.svelte" -o -name "*.json" \) -exec sha256sum {} \; 2>/dev/null || true
        find "$VENDOR_ESLINT" -type f \( -name "*.svelte" -o -name "*.json" -o -name "*.ts" -o -name "*.js" \) -exec sha256sum {} \; 2>/dev/null || true
    } | sort > "$CHECKSUM_FILE"
    echo "[verify] Wrote $(wc -l < "$CHECKSUM_FILE") checksums to $CHECKSUM_FILE"
}

# ─── Verify vendor/ hasn't been modified ────────────────────────────────────

verify_vendor() {
    if [ ! -f "$CHECKSUM_FILE" ]; then
        echo -e "${RED}[FAIL]${NC} No checksum file found. Run 'bash verify_fixtures.sh --init' first."
        return 1
    fi

    echo "[verify] Checking vendor/ integrity..."
    local current
    current=$(mktemp)
    {
        find "$VENDOR_SVELTE" -type f \( -name "*.svelte" -o -name "*.json" \) -exec sha256sum {} \; 2>/dev/null || true
        find "$VENDOR_ESLINT" -type f \( -name "*.svelte" -o -name "*.json" -o -name "*.ts" -o -name "*.js" \) -exec sha256sum {} \; 2>/dev/null || true
    } | sort > "$current"

    if ! diff -q "$CHECKSUM_FILE" "$current" > /dev/null 2>&1; then
        echo -e "${RED}[FAIL]${NC} vendor/ fixtures have been modified!"
        echo "Changed files:"
        diff "$CHECKSUM_FILE" "$current" | grep "^[<>]" | head -20
        rm "$current"
        return 1
    fi

    rm "$current"
    echo -e "${GREEN}[PASS]${NC} vendor/ fixtures are intact."
    return 0
}

# ─── Verify fixtures/ match vendor originals ───────────────────────────

verify_copied_fixtures() {
    local failures=0

    echo "[verify] Checking fixtures/ match vendor originals..."

    # Parser fixtures: compare fixtures/ copies against vendor
    if [ -d "$PARSER_FIXTURES/modern" ] && [ -d "$VENDOR_SVELTE/parser-modern/samples" ]; then
        while IFS= read -r -d '' file; do
            local relative="${file#$PARSER_FIXTURES/modern/}"
            local vendor_file="$VENDOR_SVELTE/parser-modern/samples/$relative"
            if [ -f "$vendor_file" ]; then
                if ! cmp -s "$file" "$vendor_file"; then
                    echo -e "${RED}[FAIL]${NC} Modified: $file"
                    failures=$((failures + 1))
                fi
            fi
        done < <(find "$PARSER_FIXTURES/modern" -type f -print0 2>/dev/null)
    fi

    if [ -d "$PARSER_FIXTURES/legacy" ] && [ -d "$VENDOR_SVELTE/parser-legacy/samples" ]; then
        while IFS= read -r -d '' file; do
            local relative="${file#$PARSER_FIXTURES/legacy/}"
            local vendor_file="$VENDOR_SVELTE/parser-legacy/samples/$relative"
            if [ -f "$vendor_file" ]; then
                if ! cmp -s "$file" "$vendor_file"; then
                    echo -e "${RED}[FAIL]${NC} Modified: $file"
                    failures=$((failures + 1))
                fi
            fi
        done < <(find "$PARSER_FIXTURES/legacy" -type f -print0 2>/dev/null)
    fi

    # Linter fixtures: compare fixtures/ copies against vendor
    if [ -d "$LINTER_FIXTURES" ] && [ -d "$VENDOR_ESLINT/fixtures/rules" ]; then
        while IFS= read -r -d '' file; do
            local relative="${file#$LINTER_FIXTURES/}"
            local vendor_file="$VENDOR_ESLINT/fixtures/rules/$relative"
            if [ -f "$vendor_file" ]; then
                if ! cmp -s "$file" "$vendor_file"; then
                    echo -e "${RED}[FAIL]${NC} Modified: $file"
                    failures=$((failures + 1))
                fi
            fi
        done < <(find "$LINTER_FIXTURES" -type f -print0 2>/dev/null)
    fi

    if [ "$failures" -gt 0 ]; then
        echo -e "${RED}[FAIL]${NC} $failures fixture file(s) differ from vendor originals."
        echo "Run 'bash prepare.sh' to restore them."
        return 1
    fi

    echo -e "${GREEN}[PASS]${NC} All fixtures/ match vendor originals."
    return 0
}

# ─── Main ───────────────────────────────────────────────────────────────────

case "${1:-verify}" in
    --init)
        generate_checksums
        ;;
    --verify|verify)
        verify_vendor
        vendor_ok=$?
        verify_copied_fixtures
        fixtures_ok=$?
        if [ "$vendor_ok" -ne 0 ] || [ "$fixtures_ok" -ne 0 ]; then
            echo ""
            echo -e "${RED}FIXTURE INTEGRITY CHECK FAILED.${NC}"
            echo "Tests or fixture files have been tampered with. Do NOT commit."
            exit 1
        fi
        echo ""
        echo -e "${GREEN}All fixture integrity checks passed.${NC}"
        ;;
    *)
        echo "Usage: bash verify_fixtures.sh [--init|--verify]"
        echo "  --init    Generate initial checksums after prepare.sh"
        echo "  --verify  Check that no fixtures have been modified (default)"
        exit 1
        ;;
esac
