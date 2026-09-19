#!/bin/sh
# Bring the image-owned Rust installation to the versions AGS resolved from
# upstream metadata. Unchanged parts are left alone: an unchanged compiler is
# never re-downloaded, and the rustup manager only changes when its planned
# version differs.
set -eu

: "${RUST_TRIPLE:?}" "${RUSTUP_VERSION:?}" "${RUSTC_VERSION:?}"
case "$RUST_TRIPLE" in
  x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) ;;
  *) echo "unsupported Rust host triple: $RUST_TRIPLE" >&2; exit 1 ;;
esac
case "$RUSTUP_VERSION" in
  *[!0-9.]*|'') echo "invalid rustup version: $RUSTUP_VERSION" >&2; exit 1 ;;
esac

export RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
export PATH="/usr/local/cargo/bin:$PATH"
unset RUSTUP_TOOLCHAIN

manager_version() {
  rustup --version 2>/dev/null | sed -n 's/^rustup \([0-9][0-9.]*\).*/\1/p'
}

if [ ! -x /usr/local/cargo/bin/rustup ] || [ "$(manager_version)" != "$RUSTUP_VERSION" ]; then
  echo "Installing rustup $RUSTUP_VERSION for $RUST_TRIPLE"
  work="$(mktemp -d)"
  url="https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/$RUST_TRIPLE/rustup-init"
  curl --proto '=https' --tlsv1.2 -fsSL --connect-timeout 10 --max-time 300 \
    --retry 2 --retry-delay 1 "$url" -o "$work/rustup-init"
  curl --proto '=https' --tlsv1.2 -fsSL --connect-timeout 10 --max-time 60 \
    --retry 2 --retry-delay 1 "$url.sha256" -o "$work/rustup-init.sha256"
  expected="$(cut -d ' ' -f 1 < "$work/rustup-init.sha256")"
  printf '%s  %s\n' "$expected" "$work/rustup-init" | sha256sum -c -
  chmod 0755 "$work/rustup-init"
  # `none` replaces only the manager and its proxies; toolchains stay intact.
  "$work/rustup-init" -y --no-modify-path --profile minimal --default-toolchain none
  rm -rf "$work"
fi

if [ "$(rustc +stable -V 2>/dev/null || true)" != "rustc $RUSTC_VERSION" ]; then
  echo "Installing Rust stable ($RUSTC_VERSION)"
  rustup toolchain install stable --profile minimal \
    --component rustfmt --component clippy --no-self-update
fi
rustup default stable

actual_manager="$(manager_version)"
if [ "$actual_manager" != "$RUSTUP_VERSION" ]; then
  echo "rustup version mismatch: expected $RUSTUP_VERSION, found ${actual_manager:-none}" >&2
  exit 1
fi
actual_compiler="$(rustc +stable -V)"
if [ "$actual_compiler" != "rustc $RUSTC_VERSION" ]; then
  echo "Rust stable version mismatch: expected rustc $RUSTC_VERSION, found $actual_compiler" >&2
  exit 1
fi
cargo +stable -V
rustfmt +stable --version
cargo +stable clippy --version

find /usr/local/rustup/downloads /usr/local/rustup/tmp -mindepth 1 -delete 2>/dev/null || true
chmod -R a+rX /usr/local/rustup /usr/local/cargo
