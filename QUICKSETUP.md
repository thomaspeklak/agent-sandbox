# Quick Setup

Mission: get AGS ready with less than five minutes of hands-on setup. Downloads,
compilation, and the first image build may take longer depending on your machine
and network.

## Requirements

Install these on the host first:

- Rust and Cargo
- Rootless Podman
- `git`
- `ssh-keygen`
- `ssh-add`
- `bash`

Ensure `~/.cargo/bin` is on `PATH` so the installed `ags` command is available.

## First Setup

Clone the repository and enter it:

```bash
git clone https://github.com/thomaspeklak/agent-sandbox.git && cd agent-sandbox
```

Compile and install AGS, then install or refresh its bundled assets:

```bash
cargo install --locked --path crates/ags --force && ags install
```

This command is safe to run again after pulling AGS updates.

Create the initial config and SSH setup:

```bash
ags setup
```

Choose tools by profession and area for the sandbox image:

```bash
ags tools --packages config/tool-packages.example.json
```

Build or update the sandbox image with the selected tools:

```bash
ags update-image
```

Install or update all configured agent CLIs:

```bash
ags update-agents
```

Verify the installation:

```bash
ags doctor
```

## Complete Flow

For the default tool selection, the whole process is:

```bash
git clone https://github.com/thomaspeklak/agent-sandbox.git && \
  cd agent-sandbox && \
  cargo install --locked --path crates/ags --force && \
  ags install && \
  ags setup && \
  ags update-image && \
  ags update-agents && \
  ags doctor
```

Run the tool picker before `ags update-image` when you want to customize the
sandbox tool selection.

## Clean, Build, and Install Locally

Run these commands from the repository root to remove previous build artifacts,
verify a release build, install AGS into Cargo's binary directory, and refresh
the assets embedded in the installed binary:

```bash
cargo clean
cargo build --locked --release -p ags
CARGO_TARGET_DIR=target cargo install --locked --path crates/ags --force
ags install
```

`CARGO_TARGET_DIR=target` lets `cargo install` reuse the release artifacts from
the explicit build. Cargo normally installs the `ags` binary in
`~/.cargo/bin`; ensure that directory is on `PATH`. Run `ags install` after each
source installation so later image builds use the matching embedded
`Containerfile` and configuration assets.

Confirm that the local installation is available:

```bash
command -v ags
ags --help
ags doctor
```

On a new machine, run `ags setup` before rebuilding the sandbox image. Then
verify the image-provided pnpm launcher and refresh the installed agents:

```bash
ags update-image
ags --agent shell -- -lc 'test -L /usr/local/bin/pnpm && /usr/local/bin/pnpm --version'
ags update-agents
ags doctor
```

## Updating Later

From the cloned repository, update AGS itself with one repeatable command:

```bash
git pull --ff-only && cargo install --locked --path crates/ags --force && ags install
```

Refresh the image and agent CLIs after an AGS update:

```bash
ags update-image && ags update-agents
```

Reopen the tool picker whenever you want to change sandbox tools:

```bash
ags tools --packages config/tool-packages.example.json
```
