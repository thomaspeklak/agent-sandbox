# Quick Setup

Mission: get AGS ready with less than five minutes of hands-on setup. Downloads,
compilation, and the first image build may take longer depending on your machine
and network.

## Requirements

Install these on the host first:

- Rust and Cargo
- Rootless Podman
- Git

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
