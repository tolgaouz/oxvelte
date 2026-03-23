#!/usr/bin/env bash
set -euo pipefail

# Build oxvelte binaries for all npm platforms and stage them for publishing.
#
# Usage:
#   ./scripts/build-npm.sh          # build all targets
#   ./scripts/build-npm.sh publish  # build + npm publish
#
# Prerequisites:
#   - Rust cross-compilation targets installed
#   - cargo-zigbuild (for linux cross-compile) or cross

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "Building oxvelte v${VERSION}"

TARGETS=(
  "aarch64-apple-darwin|cli-darwin-arm64|oxvelte"
  "x86_64-apple-darwin|cli-darwin-x64|oxvelte"
  "x86_64-unknown-linux-gnu|cli-linux-x64-gnu|oxvelte"
  "aarch64-unknown-linux-gnu|cli-linux-arm64-gnu|oxvelte"
  "x86_64-pc-windows-msvc|cli-win32-x64-msvc|oxvelte.exe"
)

# Update version in all package.json files
for dir in npm/oxvelte npm/@oxvelte/cli-*; do
  if [ -f "$dir/package.json" ]; then
    sed -i.bak "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" "$dir/package.json"
    rm -f "$dir/package.json.bak"
  fi
done

# Also update optionalDependencies versions in main package
for target_info in "${TARGETS[@]}"; do
  IFS='|' read -r target pkg_name bin_name <<< "$target_info"
  sed -i.bak "s|\"@oxvelte/${pkg_name}\": \".*\"|\"@oxvelte/${pkg_name}\": \"${VERSION}\"|" npm/oxvelte/package.json
  rm -f npm/oxvelte/package.json.bak
done

# Build for each target
for target_info in "${TARGETS[@]}"; do
  IFS='|' read -r target pkg_name bin_name <<< "$target_info"
  echo ""
  echo "=== Building for ${target} ==="

  if cargo build --release --target "${target}" 2>/dev/null; then
    cp "target/${target}/release/${bin_name}" "npm/@oxvelte/${pkg_name}/${bin_name}"
    echo "  -> npm/@oxvelte/${pkg_name}/${bin_name}"
  else
    echo "  SKIP (target not installed: ${target})"
  fi
done

# Copy README to main npm package
cp README.md npm/oxvelte/README.md

echo ""
echo "Build complete. Packages staged in npm/"

# Publish if requested
if [ "${1:-}" = "publish" ]; then
  echo ""
  echo "Publishing to npm..."

  # Publish platform packages first
  for target_info in "${TARGETS[@]}"; do
    IFS='|' read -r target pkg_name bin_name <<< "$target_info"
    pkg_dir="npm/@oxvelte/${pkg_name}"
    if [ -f "${pkg_dir}/${bin_name}" ]; then
      echo "  Publishing @oxvelte/${pkg_name}..."
      (cd "${pkg_dir}" && npm publish --access public)
    fi
  done

  # Publish main package last
  echo "  Publishing oxvelte..."
  (cd npm/oxvelte && npm publish --access public)

  echo ""
  echo "Published oxvelte@${VERSION}"
fi
