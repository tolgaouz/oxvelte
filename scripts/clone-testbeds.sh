#!/usr/bin/env bash
# Clone the parity-check testbeds into <repo>/testbeds/<name>.
# (testbeds/ is gitignored — this script is the source of truth for which
# repos make up the parity corpus.)
#
# Usage:
#   ./scripts/clone-testbeds.sh                # clone everything missing
#   ./scripts/clone-testbeds.sh kit immich     # clone only listed testbeds
#   ./scripts/clone-testbeds.sh --list         # print the manifest and exit
#   ./scripts/clone-testbeds.sh --list-skipped # print excluded testbeds/reasons
#   ./scripts/clone-testbeds.sh --no-pin       # clone HEAD even when a pin is set
#   ./scripts/clone-testbeds.sh --no-submodules
#   ./scripts/clone-testbeds.sh --force        # re-clone (rm -rf) existing dirs
#
# Always does FULL clones (no --depth) so parity checks see realistic, complete
# codebases. Pins make numbers reproducible; bump them deliberately to refresh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TESTBEDS_DIR="$ROOT/testbeds"
MANIFEST_FILE="$SCRIPT_DIR/testbeds.tsv"
SKIPPED_MANIFEST_FILE="$SCRIPT_DIR/testbeds-skipped.tsv"
mkdir -p "$TESTBEDS_DIR"

no_pin=0
force=0
submodules=1
list_only=0
list_skipped=0
selected=()

for arg in "$@"; do
  case "$arg" in
    --no-pin)   no_pin=1 ;;
    --no-submodules) submodules=0 ;;
    --force)    force=1 ;;
    --list)     list_only=1 ;;
    --list-skipped) list_skipped=1 ;;
    -h|--help)  sed -n '2,18p' "$0"; exit 0 ;;
    --*)        echo "unknown flag: $arg" >&2; exit 2 ;;
    *)          selected+=("$arg") ;;
  esac
done

pin_label() {
  if [ -z "${1:-}" ] || [ "$1" = "-" ]; then
    printf '(HEAD)'
  else
    printf '%s' "$1"
  fi
}

if [ "$list_only" = 1 ]; then
  printf '%-22s %-70s %-40s %-14s %s\n' name url pin package_root eslint_root
  printf '%-22s %-70s %-40s %-14s %s\n' "$(printf -- '-%.0s' {1..22})" "$(printf -- '-%.0s' {1..70})" "$(printf -- '-%.0s' {1..40})" "$(printf -- '-%.0s' {1..14})" "$(printf -- '-%.0s' {1..14})"
  while IFS=$'\t' read -r name url pin package_root eslint_root; do
    case "$name" in ''|\#*) continue ;; esac
    printf '%-22s %-70s %-40s %-14s %s\n' "$name" "$url" "$(pin_label "$pin")" "${package_root:-.}" "${eslint_root:-.}"
  done < "$MANIFEST_FILE"
  exit 0
fi

if [ "$list_skipped" = 1 ]; then
  printf '%-22s %-28s %s\n' name status reason
  printf '%-22s %-28s %s\n' "$(printf -- '-%.0s' {1..22})" "$(printf -- '-%.0s' {1..28})" "$(printf -- '-%.0s' {1..70})"
  while IFS=$'\t' read -r name url pin package_root eslint_root status reason; do
    case "$name" in ''|\#*) continue ;; esac
    printf '%-22s %-28s %s\n' "$name" "${status:-unknown}" "${reason:-}"
  done < "$SKIPPED_MANIFEST_FILE"
  exit 0
fi

want() {
  [ ${#selected[@]} -eq 0 ] && return 0
  for s in "${selected[@]}"; do [ "$s" = "$1" ] && return 0; done
  return 1
}

ok=(); skipped=(); failed=(); matched_selected=()

skipped_reason() {
  local target="$1"
  while IFS=$'\t' read -r name url pin package_root eslint_root status reason; do
    case "$name" in ''|\#*) continue ;; esac
    if [ "$name" = "$target" ]; then
      printf '%s\t%s\n' "${status:-unknown}" "${reason:-}"
      return 0
    fi
  done < "$SKIPPED_MANIFEST_FILE"
  return 1
}

was_matched() {
  local target="$1"
  [ "${#matched_selected[@]}" -gt 0 ] || return 1
  for name in "${matched_selected[@]}"; do
    [ "$name" = "$target" ] && return 0
  done
  return 1
}

verify_full_clone() {
  local name="$1" dest="$2" shallow
  if [ ! -d "$dest/.git" ]; then
    echo "!! $name: $dest is not a git clone" >&2
    return 1
  fi
  shallow="$(git -C "$dest" rev-parse --is-shallow-repository 2>/dev/null || printf unknown)"
  if [ "$shallow" != "false" ]; then
    echo "!! $name: expected a full clone, got shallow=$shallow" >&2
    return 1
  fi
}

init_submodules() {
  local name="$1" dest="$2"
  [ "$submodules" = 1 ] || return 0
  if git -C "$dest" config --file .gitmodules --get-regexp path >/dev/null 2>&1; then
    echo "   $name: initializing submodules"
    git -C "$dest" submodule update --init --recursive --quiet
  fi
}

clone_one() {
  local name="$1" url="$2" pin="$3"
  local dest="$TESTBEDS_DIR/$name"

  if [ -e "$dest" ]; then
    if [ "$force" = 1 ]; then
      echo ">> $name: --force, removing existing $dest"
      rm -rf "$dest"
    else
      echo "== $name: exists, skipping (use --force to re-clone)"
      if ! verify_full_clone "$name" "$dest"; then
        failed+=("$name")
        return 1
      fi
      skipped+=("$name")
      return 0
    fi
  fi

  echo ">> $name: cloning $url"
  if ! git clone --quiet "$url" "$dest"; then
    failed+=("$name"); return 1
  fi
  if [ -n "$pin" ] && [ "$pin" != "-" ] && [ "$no_pin" = 0 ]; then
    if ! git -C "$dest" checkout --quiet "$pin"; then
      echo "!! $name: failed to checkout $pin (commit not in default branch?)" >&2
      failed+=("$name"); return 1
    fi
    echo "   $name: full clone, pinned at ${pin:0:12}"
  else
    echo "   $name: full clone @ HEAD"
  fi
  if ! verify_full_clone "$name" "$dest"; then
    failed+=("$name"); return 1
  fi
  if ! init_submodules "$name" "$dest"; then
    failed+=("$name"); return 1
  fi
  ok+=("$name")
}

while IFS=$'\t' read -r name url pin package_root eslint_root; do
  case "$name" in ''|\#*) continue ;; esac
  want "$name" || continue
  matched_selected+=("$name")
  clone_one "$name" "$url" "$pin" || true
done < "$MANIFEST_FILE"

if [ ${#selected[@]} -gt 0 ]; then
  for name in "${selected[@]}"; do
    was_matched "$name" && continue
    if info="$(skipped_reason "$name")"; then
      status="${info%%$'\t'*}"
      reason="${info#*$'\t'}"
      echo "!! $name: skipped ($status) $reason" >&2
    else
      echo "!! $name: unknown testbed" >&2
    fi
    failed+=("$name")
  done
fi

echo
echo "── summary ──────────────────────────────"
echo "cloned:  ${#ok[@]}      ${ok[*]:-}"
echo "skipped: ${#skipped[@]}      ${skipped[*]:-}"
echo "failed:  ${#failed[@]}      ${failed[*]:-}"
[ ${#failed[@]} -eq 0 ]
