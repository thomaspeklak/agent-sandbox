# Smoke test for a candidate sandbox image. AGS runs it with networking
# disabled and no host mounts, credentials, workspaces, or agent volumes before
# the candidate may replace the configured image.
set -eu

fail() {
  echo "verify-image: $*" >&2
  exit 1
}

: "${EXPECT_PNPM_VERSION:?}" "${EXPECT_RUSTC_VERSION:?}" "${EXPECT_RUSTUP_VERSION:?}"
: "${EXPECT_RPMS:?}"
EXPECT_COMMANDS="${EXPECT_COMMANDS:-}"

[ "$(id -un)" = dev ] || fail "default user is $(id -un), expected dev"
[ "$(pwd)" = /workspace ] || fail "working directory is $(pwd), expected /workspace"
[ "$HOME" = /home/dev ] || fail "HOME is $HOME, expected /home/dev"

for path in /etc/uv/uv.toml /home/dev/.tmux.conf /home/dev/.config/pnpm/config.yaml \
  /opt/ags/rustc-wrapper /opt/ags/glimpse-shim; do
  [ -e "$path" ] || fail "missing $path"
done
grep -qx 'ignore-scripts=true' /etc/npmrc || fail "/etc/npmrc does not disable npm scripts"
grep -qx 'ignore-scripts=true' /home/dev/.npmrc || fail "~/.npmrc does not disable npm scripts"
grep -qx 'ignoreScripts: true' /home/dev/.config/pnpm/config.yaml || fail "pnpm scripts are not disabled"

# --whatprovides also accepts selections DNF resolved through a provide.
missing=""
for package in $EXPECT_RPMS; do
  rpm -q --whatprovides "$package" >/dev/null 2>&1 || missing="$missing $package"
done
[ -z "$missing" ] || fail "selected RPMs are missing:$missing"

for command in $EXPECT_COMMANDS; do
  [ -x "/usr/local/bin/$command" ] || fail "missing vendor tool /usr/local/bin/$command"
done

command -v mise >/dev/null || fail "mise is missing"
node --version >/dev/null || fail "node does not run"
[ -L /usr/local/bin/pnpm ] || fail "/usr/local/bin/pnpm is not the package launcher symlink"
actual_pnpm="$(/usr/local/bin/pnpm --version)"
[ "$actual_pnpm" = "$EXPECT_PNPM_VERSION" ] || fail "pnpm $actual_pnpm, expected $EXPECT_PNPM_VERSION"

actual_rustc="$(rustc -V)"
[ "$actual_rustc" = "rustc $EXPECT_RUSTC_VERSION" ] || fail "$actual_rustc, expected rustc $EXPECT_RUSTC_VERSION"
[ "$(rustc +stable -V)" = "$actual_rustc" ] || fail "rustc +stable differs from the default toolchain"
rustup --version 2>/dev/null | grep -q "^rustup $EXPECT_RUSTUP_VERSION " || fail "rustup is not $EXPECT_RUSTUP_VERSION"
cargo -V >/dev/null || fail "cargo does not run"
rustfmt --version >/dev/null || fail "rustfmt does not run"
cargo clippy --version >/dev/null || fail "clippy does not run"
/opt/ags/glimpse-shim --help >/dev/null || fail "glimpse-shim does not run"

project="$(mktemp -d)"
mkdir "$project/src"
printf '[package]\nname = "ags-verify"\nversion = "0.0.0"\nedition = "2021"\n' > "$project/Cargo.toml"
printf 'fn main() { println!("ok"); }\n' > "$project/src/main.rs"
cd "$project"
cargo build --offline --quiet || fail "offline cargo build failed with RUSTC_WRAPPER=$RUSTC_WRAPPER"
[ "$(./target/debug/ags-verify)" = ok ] || fail "compiled program printed unexpected output"
cargo clean --quiet
RUSTC_WRAPPER= cargo build --offline --quiet || fail "offline cargo build failed without RUSTC_WRAPPER"
cd /
rm -rf "$project"
echo "verify-image: ok"
