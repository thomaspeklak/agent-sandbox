#!/usr/bin/env sh
set -eu

# Cargo invokes rustc-wrapper as:
#   <wrapper> <path-to-rustc> <rustc-args...>
#
# Prefer sccache when available, but gracefully fall back to direct rustc
# execution when sccache is not installed (e.g. on CI runners).
if command -v sccache >/dev/null 2>&1; then
  exec sccache "$@"
fi

exec "$@"
