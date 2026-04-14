#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template="${repo_root}/PKGBUILD.in"
output="${repo_root}/PKGBUILD"
cargo_toml="${repo_root}/crates/ags/Cargo.toml"
upstream_repo="thomaspeklak/agent-sandbox"

version="${1:-}"
if [[ -z "${version}" ]]; then
  version="$(sed -n 's/^version = "\(.*\)"/\1/p' "${cargo_toml}" | head -n 1)"
fi

if [[ -z "${version}" ]]; then
  echo "failed to determine version from ${cargo_toml}" >&2
  exit 1
fi

if [[ ! -f "${template}" ]]; then
  echo "missing template: ${template}" >&2
  exit 1
fi

tarball="https://github.com/${upstream_repo}/archive/refs/tags/v${version}.tar.gz"
sha256="$(curl -fsSL "${tarball}" | sha256sum | awk '{print $1}')"

sed \
  -e "s/@PKGVER@/${version}/g" \
  -e "s/@SHA256@/${sha256}/g" \
  "${template}" > "${output}"

echo "rendered ${output} for v${version}"
