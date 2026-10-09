#!/bin/sh
set -eu
@@T3_NODE_ENV_SCRIPT@@
T3_NODE_SCRIPT_PATH=@@T3_NODE_SCRIPT_PATH@@
if [ -n "$T3_NODE_SCRIPT_PATH" ]; then
  ensure_remote_node_path || true
  if ! command -v node >/dev/null 2>&1; then
    printf 'Remote host is missing node on PATH. Install Node or configure a supported version manager for non-interactive shells.\n' >&2
    exit 1
  fi
  exec node "$T3_NODE_SCRIPT_PATH" "$@"
fi
T3_ARCHIVE_VERSION=@@T3_ARCHIVE_VERSION@@
if [ -z "$T3_ARCHIVE_VERSION" ]; then
  printf 'No t3 release version was provided for the remote runtime.\n' >&2
  exit 1
fi
T3_RELEASE_BASE_URL=@@T3_RELEASE_BASE_URL@@
T3_RUNTIME_DIR="$HOME/.t3/runtime/versions/$T3_ARCHIVE_VERSION"
t3_runtime_ready() {
  [ -x "$T3_RUNTIME_DIR/t3" ] && [ "$(cat "$T3_RUNTIME_DIR/.install-complete" 2>/dev/null)" = "$T3_ARCHIVE_VERSION" ]
}
if ! t3_runtime_ready; then
  mkdir -p "$HOME/.t3/runtime/versions"
  T3_LOCK="$HOME/.t3/runtime/versions/.$T3_ARCHIVE_VERSION.install.lock"
  T3_LOCK_WAITED=0
  T3_LOCK_UNOWNED=0
  while ! mkdir "$T3_LOCK" 2>/dev/null; do
    T3_LOCK_OWNER="$(cat "$T3_LOCK/pid" 2>/dev/null || true)"
    if [ -n "$T3_LOCK_OWNER" ]; then
      T3_LOCK_UNOWNED=0
      if ! kill -0 "$T3_LOCK_OWNER" 2>/dev/null; then
        rm -rf "$T3_LOCK"
        continue
      fi
    else
      T3_LOCK_UNOWNED=$((T3_LOCK_UNOWNED + 1))
      if [ "$T3_LOCK_UNOWNED" -ge 5 ]; then
        rm -rf "$T3_LOCK"
        continue
      fi
    fi
    if [ "$T3_LOCK_WAITED" -ge 360 ]; then
      printf 'Another t3 %s installation has held %s for too long.\n' "$T3_ARCHIVE_VERSION" "$T3_LOCK" >&2
      exit 1
    fi
    sleep 1
    T3_LOCK_WAITED=$((T3_LOCK_WAITED + 1))
  done
  printf '%s\n' "$$" > "$T3_LOCK/pid.tmp" && mv "$T3_LOCK/pid.tmp" "$T3_LOCK/pid"
  trap 'rm -rf "$T3_LOCK"' EXIT
fi
if ! t3_runtime_ready; then
  case "$(uname -s)" in
    Darwin) T3_PLATFORM="darwin" ;;
    Linux) T3_PLATFORM="linux" ;;
    *) printf 'Remote host %s has no t3 release archive.\n' "$(uname -s)" >&2; exit 1 ;;
  esac
  case "$(uname -m)" in
    arm64 | aarch64) T3_ARCH="arm64" ;;
    x86_64 | amd64) T3_ARCH="x64" ;;
    *) printf 'Remote host %s has no t3 release archive.\n' "$(uname -m)" >&2; exit 1 ;;
  esac
  T3_ARCHIVE="t3-$T3_ARCHIVE_VERSION-$T3_PLATFORM-$T3_ARCH.tar.gz"
  T3_STAGING="$(mktemp -d "$HOME/.t3/runtime/versions/.staging-XXXXXX")"
  trap 'rm -rf "$T3_STAGING" "$T3_LOCK"' EXIT
  t3_fetch() {
    if command -v curl >/dev/null 2>&1; then curl -fsSL --connect-timeout 30 --max-time "$3" "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then wget -q --timeout=30 --tries=1 "$1" -O "$2"
    else printf 'Remote host needs curl or wget to download %s.\n' "$T3_ARCHIVE" >&2; exit 1
    fi
  }
  t3_fetch "$T3_RELEASE_BASE_URL/v$T3_ARCHIVE_VERSION/SHA256SUMS" "$T3_STAGING/SHA256SUMS" 30
  t3_fetch "$T3_RELEASE_BASE_URL/v$T3_ARCHIVE_VERSION/$T3_ARCHIVE" "$T3_STAGING/$T3_ARCHIVE" 240
  T3_EXPECTED="$(grep " \*\{0,1\}$T3_ARCHIVE$" "$T3_STAGING/SHA256SUMS" | cut -d' ' -f1)"
  if command -v sha256sum >/dev/null 2>&1; then
    T3_ACTUAL="$(sha256sum "$T3_STAGING/$T3_ARCHIVE" | cut -d' ' -f1)"
  else
    T3_ACTUAL="$(shasum -a 256 "$T3_STAGING/$T3_ARCHIVE" | cut -d' ' -f1)"
  fi
  if [ -z "$T3_EXPECTED" ] || [ "$T3_ACTUAL" != "$T3_EXPECTED" ]; then
    printf 'Checksum mismatch for %s.\n' "$T3_ARCHIVE" >&2; exit 1
  fi
  tar -xzf "$T3_STAGING/$T3_ARCHIVE" -C "$T3_STAGING" --strip-components=1
  rm -f "$T3_STAGING/$T3_ARCHIVE" "$T3_STAGING/SHA256SUMS"
  if ! "$T3_STAGING/t3" --version >/dev/null 2>&1; then
    printf 'The t3 %s executable does not run on this host.\n' "$T3_ARCHIVE_VERSION" >&2; exit 1
  fi
  printf '%s\n' "$T3_ARCHIVE_VERSION" > "$T3_STAGING/.install-complete"
  rm -rf "$T3_RUNTIME_DIR"
  mv "$T3_STAGING" "$T3_RUNTIME_DIR"
fi
if [ -n "${T3_LOCK:-}" ]; then
  rm -rf "$T3_LOCK"
  trap - EXIT
fi
exec "$T3_RUNTIME_DIR/t3" "$@"
