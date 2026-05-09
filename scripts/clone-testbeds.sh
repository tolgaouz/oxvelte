#!/usr/bin/env bash
# Clone the parity-check testbeds into <repo>/testbeds/<name>.
# (testbeds/ is gitignored — this script is the source of truth for which
# repos make up the parity corpus.)
#
# Usage:
#   ./scripts/clone-testbeds.sh                # clone everything missing
#   ./scripts/clone-testbeds.sh kit immich     # clone only listed testbeds
#   ./scripts/clone-testbeds.sh --list         # print the manifest and exit
#   ./scripts/clone-testbeds.sh --no-pin       # clone HEAD even when a pin is set
#   ./scripts/clone-testbeds.sh --force        # re-clone (rm -rf) existing dirs
#
# Always does FULL clones (no --depth) so parity checks see realistic, complete
# codebases. Pins make numbers reproducible; bump them deliberately to refresh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TESTBEDS_DIR="$ROOT/testbeds"
mkdir -p "$TESTBEDS_DIR"

# name <TAB> url <TAB> pinned-commit (empty = track HEAD)
read -r -d '' MANIFEST <<'EOF' || true
adventurelog	https://github.com/seanmorley15/AdventureLog.git
appwrite-console	https://github.com/appwrite/console.git	e5b241c56379cb77eb8a445b1e419f258f33f1d7
bits-ui	https://github.com/huntabyte/bits-ui.git	f4fc53225721afa9a798eb21501c9f40bf2e375b
carbon	https://github.com/carbon-design-system/carbon-components-svelte.git
cobalt	https://github.com/imputnet/cobalt.git
dbgate	https://github.com/dbgate/dbgate.git
flowbite-svelte	https://github.com/themesberg/flowbite-svelte.git	2c1d2e0a84604374dd1d3e6b7bc8d6c775011be4
huly	https://github.com/hcengineering/platform.git	cc16352be93223dbaaf35efd9548a5d75d79ef62
immich	https://github.com/immich-app/immich.git	00dae6ac3896f2528a3edac8f489877bd58ceb52
kener	https://github.com/rajnandan1/kener.git
kit	https://github.com/sveltejs/kit.git	b31efce05cbc5929101643da80b837c270c63fd8
open-webui	https://github.com/open-webui/open-webui.git	e4e69a10ec08a725bf2ab3db499ef664f2bd7570
shadcn-svelte	https://github.com/huntabyte/shadcn-svelte.git	28c320ccaa2ef225e1eb830c5b964593a6eff4c4
skeleton	https://github.com/skeletonlabs/skeleton.git
smui	https://github.com/hperrin/svelte-material-ui.git
sveltekit-superforms	https://github.com/ciscoheat/sveltekit-superforms.git	b54f69f8ccdfad2ac62d9f7383663c9723f42469
threlte	https://github.com/threlte/threlte.git
windmill	https://github.com/windmill-labs/windmill.git	ef1757f5d747e513d201eb6fa48918dba8248abe
EOF

no_pin=0
force=0
list_only=0
selected=()

for arg in "$@"; do
  case "$arg" in
    --no-pin)   no_pin=1 ;;
    --force)    force=1 ;;
    --list)     list_only=1 ;;
    -h|--help)  sed -n '2,16p' "$0"; exit 0 ;;
    --*)        echo "unknown flag: $arg" >&2; exit 2 ;;
    *)          selected+=("$arg") ;;
  esac
done

if [ "$list_only" = 1 ]; then
  printf '%-22s %-70s %s\n' name url pin
  printf '%-22s %-70s %s\n' "$(printf -- '-%.0s' {1..22})" "$(printf -- '-%.0s' {1..70})" "$(printf -- '-%.0s' {1..40})"
  while IFS=$'\t' read -r name url pin; do
    [ -z "$name" ] && continue
    printf '%-22s %-70s %s\n' "$name" "$url" "${pin:-(HEAD)}"
  done <<< "$MANIFEST"
  exit 0
fi

want() {
  [ ${#selected[@]} -eq 0 ] && return 0
  for s in "${selected[@]}"; do [ "$s" = "$1" ] && return 0; done
  return 1
}

ok=(); skipped=(); failed=()

clone_one() {
  local name="$1" url="$2" pin="$3"
  local dest="$TESTBEDS_DIR/$name"

  if [ -e "$dest" ]; then
    if [ "$force" = 1 ]; then
      echo ">> $name: --force, removing existing $dest"
      rm -rf "$dest"
    else
      echo "== $name: exists, skipping (use --force to re-clone)"
      skipped+=("$name")
      return 0
    fi
  fi

  echo ">> $name: cloning $url"
  if ! git clone --quiet "$url" "$dest"; then
    failed+=("$name"); return 1
  fi
  if [ -n "$pin" ] && [ "$no_pin" = 0 ]; then
    if ! git -C "$dest" checkout --quiet "$pin"; then
      echo "!! $name: failed to checkout $pin (commit not in default branch?)" >&2
      failed+=("$name"); return 1
    fi
    echo "   $name: full clone, pinned at ${pin:0:12}"
  else
    echo "   $name: full clone @ HEAD"
  fi
  ok+=("$name")
}

while IFS=$'\t' read -r name url pin; do
  [ -z "$name" ] && continue
  want "$name" || continue
  clone_one "$name" "$url" "$pin" || true
done <<< "$MANIFEST"

echo
echo "── summary ──────────────────────────────"
echo "cloned:  ${#ok[@]}      ${ok[*]:-}"
echo "skipped: ${#skipped[@]}      ${skipped[*]:-}"
echo "failed:  ${#failed[@]}      ${failed[*]:-}"
[ ${#failed[@]} -eq 0 ]
