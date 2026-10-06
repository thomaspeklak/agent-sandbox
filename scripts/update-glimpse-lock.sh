#!/usr/bin/env bash
# Regenerate config/image/glimpse-shim.Cargo.lock, the standalone lockfile the
# sandbox image uses to build the Glimpse shim with `cargo build --locked`.
# It starts from the workspace lock so every version stays identical, and lets
# Cargo prune the entries the shim does not use.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cp -R "$repo/crates/glimpse-shim/." "$work/"
rm -rf "$work/target"
cp "$repo/Cargo.lock" "$work/Cargo.lock"
# Resolution without --locked prunes unused packages but keeps pinned versions.
(cd "$work" && cargo tree --offline -e normal,build,dev >/dev/null)
(cd "$work" && cargo tree --locked --offline -e normal,build,dev >/dev/null)
cp "$work/Cargo.lock" "$repo/config/image/glimpse-shim.Cargo.lock"
echo "Updated config/image/glimpse-shim.Cargo.lock"
