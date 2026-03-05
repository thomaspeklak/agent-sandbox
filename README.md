# agent-sandbox

![agent logos in a sandbox](./agent-sandbox-logo.webp)

`ags` is a Rust CLI that launches AI coding agents inside a rootless Podman sandbox.

It is designed to keep your host clean while still giving agents controlled access to your repo, selected tools, and selected secrets.

## Documentation map

- `README.md` (this file): quick start + daily usage
- `docs/COMMANDS.md`: detailed command behavior and side effects
- `docs/CONFIG.md`: full config schema and semantics
- `docs/TROUBLESHOOTING.md`: common problems and fixes
- `docs/ARCHITECTURE.md`: internal architecture overview
- `CONTRIBUTING.md`: contributor workflow and quality checklist

## What this tool provides

- Rootless containerized agent runs (Podman)
- Multi-agent support:
  - `pi`
  - `claude`
  - `codex`
  - `gemini`
  - `opencode`
  - `shell` (interactive bash with agent environments mounted)
- First-run setup for SSH auth + signing keys
- Persistent per-agent host volumes (sessions/config survive container restarts)
- Configurable mounts, tool binaries, and secret sources
- Optional browser sidecar support for browser-enabled workflows
- Health checks via `ags doctor`
- Convenience alias/wrapper generation via `ags create-aliases`

---

## Requirements

Required on host:

- Rust toolchain (to build/run `ags` from source)
- Podman (rootless recommended)
- `git`
- `ssh-keygen`
- `ssh-add`
- `bash`

Optional but useful:

- `make` (for convenience targets)
- `secret-tool` (GNOME keyring/libsecret integration)
- Browser executable (for `--browser` mode)

> Tip: run `ags doctor` after setup to verify your environment.

---

## Build and install

From repository root:

```bash
# build debug binary
cargo build -p ags

# build optimized release binary
cargo build -p ags --release
```

Run without installing:

```bash
cargo run -p ags -- --agent pi
```

Optional self-link into `~/.local/bin/ags`:

```bash
cargo run -p ags -- install --link-self
```

If an existing file/symlink should be replaced:

```bash
cargo run -p ags -- install --link-self --force
```

You can also use Make targets (see below).

---

## First-time setup

### 1) Install baseline assets and config layout

```bash
cargo run -p ags -- install
```

This writes:

- `~/.config/ags/Containerfile`
- `~/.config/ags/pi/extensions/guard.ts`
- `~/.config/ags/pi/settings.json` (if missing)

### 2) Create and edit config

Use `config/config.example.toml` as your template:

```bash
mkdir -p ~/.config/ags
cp config/config.example.toml ~/.config/ags/config.toml
```

Then replace placeholders with real values and paths.

### 3) Run setup

```bash
cargo run -p ags -- setup
```

`setup` will:

- Generate SSH keys if missing:
  - `~/.ssh/ags-agent-auth`
  - `~/.ssh/ags-agent-signing`
- Print public keys so you can add them to GitHub
- Bootstrap per-agent sandbox directories
- Optionally prompt to store configured secrets via `secret-tool`

### 4) Build/update sandbox image and agent installs

```bash
cargo run -p ags -- update
cargo run -p ags -- update-agents
```

### 5) Verify

```bash
cargo run -p ags -- doctor
```

---

## Quick start (Makefile)

Equivalent convenience flow:

```bash
make setup
make doctor
make update
make update-agents
make run
```

Available targets:

- `make setup`
- `make doctor`
- `make update`
- `make update-agents`
- `make run`
- `make run-browser`
- `make install`
- `make install-self`
- `make uninstall`
- `make aliases`

---

## Daily usage

Run Pi agent:

```bash
ags --agent pi
```

Run with browser sidecar:

```bash
ags --agent pi --browser
```

Run other agents:

```bash
ags --agent claude
ags --agent codex
ags --agent gemini
ags --agent opencode
ags --agent shell
```

Pass arguments through to the underlying agent CLI using `--`:

```bash
ags --agent pi -- --continue
ags --agent claude -- --model sonnet
```

Use a non-default config file:

```bash
ags --agent pi --config /path/to/config.toml
```

---

## Commands reference

### Core commands

- `ags setup` — generate keys, bootstrap agent sandboxes, optional keyring secret setup
- `ags doctor` — run environment + config health checks
- `ags update` — rebuild container image from `Containerfile`
- `ags update-agents` — install/update agent CLIs in persistent volumes
- `ags install [--link-self] [--force]` — install assets/config layout, optional self-link
- `ags uninstall` — currently reserved/no-op cleanup
- `ags create-aliases` — create managed wrappers and/or shell alias blocks

### `create-aliases` options

```bash
ags create-aliases --mode wrappers|aliases|both --shell fish|zsh|bash --force
```

- default mode: `wrappers`
- if `--shell` omitted, shell is autodetected from `$SHELL`

### Global run flags

- `--agent <pi|claude|codex|gemini|opencode|shell>` (required for run mode)
- `--browser`
- `--config <path>`

---

## Configuration guide

Default config path:

- `~/.config/ags/config.toml`

If missing, `ags` auto-creates a minimal default config on first run.

Use `config/config.example.toml` for full schema examples.

### Important sections

- `[sandbox]`
  - Container image name and core paths
  - SSH key paths
  - bootstrap files
  - base env passthrough allowlist
- `[[mount]]`
  - Bind mounts from host to container
  - supports `mode`, `kind`, `when`, `create`, `optional`
- `[[tool]]`
  - Tool binary mount
  - optional nested `[[tool.directory]]` mounts
  - optional nested `[[tool.secret]]` sources
- `[[secret]]`
  - Map env var names to source(s): `from_env` and/or `secret_store`
- `[browser]`
  - Enables browser sidecar integration used with `--browser`
- `[update]`
  - Controls Pi package spec and pnpm minimum release age for updates

---

## Security notes

- Use least-privilege, short-lived tokens whenever possible.
- Only mount what the agent needs.
- Prefer read-only (`ro`) mounts unless write access is required.
- Treat `passthrough_env` and configured secrets as sensitive data paths.
- Rotate/revoke credentials quickly if compromise is suspected.

---

## Project layout

- `crates/ags/` — Rust CLI implementation
- `config/Containerfile` — base sandbox image definition
- `config/config.example.toml` — full config template
- `agent/extensions/guard.ts` — runtime guard extension mounted for Pi
- `agent/settings.example.json` — example Pi settings template
- `Makefile` — convenience command wrappers

---

## Troubleshooting

- Run `ags doctor` first.
- If image is missing/stale: run `ags update`.
- If agent CLIs are missing/stale: run `ags update-agents`.
- If browser mode fails:
  - ensure `[browser].enabled = true`
  - verify `[browser].command` is valid
  - verify debug port is available
- If secrets are not found:
  - verify env vars exist and are non-empty
  - or verify `secret-tool` entries match configured attributes

---

## Contributing

Contributions are welcome.

For the full contributor guide, see [`CONTRIBUTING.md`](./CONTRIBUTING.md).

### Development setup

```bash
# build
cargo build -p ags

# format
cargo fmt

# lint
cargo clippy -p ags -- -D warnings

# test
cargo test -p ags
```

### Suggested PR workflow

1. Create a focused branch.
2. Make small, clear commits.
3. Add/update tests for behavior changes.
4. Run fmt/clippy/tests locally.
5. Update docs (README/config example/help text) when behavior changes.
6. Open PR with:
   - summary of user-visible changes
   - config/migration notes (if any)
   - test coverage notes

### High-value contribution areas

- Better diagnostics in `doctor`
- Config schema/documentation improvements
- Additional agent/profile support
- Better cross-platform behavior and install UX
- Safer defaults and security hardening
