# Local Source Build and Install

Run these commands from the repository root to clean previous build artifacts,
compile AGS from source, and install it locally:

```bash
cargo clean
cargo build --locked --release -p ags
CARGO_TARGET_DIR=target cargo install --locked --path crates/ags --force
ags install
```

`cargo build` verifies the release build. `cargo install` installs the `ags`
binary into Cargo's binary directory, normally `~/.cargo/bin`. Ensure that
directory is on `PATH`.

`ags install` refreshes the configuration assets embedded in the newly built
binary, including the sandbox `Containerfile`. Run it after installing a new
source build so later image builds use the matching assets.

## Verify the Local Install

Confirm that the installed binary is available:

```bash
command -v ags
ags --help
ags doctor
```

To rebuild the sandbox image and refresh the installed agent CLIs from the new
source build:

```bash
ags update-image
ags --agent shell -- -lc 'test -L /usr/local/bin/pnpm && /usr/local/bin/pnpm --version'
ags update-agents
ags doctor
```

Run `ags setup` before these verification commands when configuring AGS on a
new machine for the first time.

## Rebuild After Changes

For later source changes, repeat the same clean installation flow:

```bash
cargo clean && \
  cargo build --locked --release -p ags && \
  CARGO_TARGET_DIR=target cargo install --locked --path crates/ags --force && \
  ags install
```
