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
- Auth proxy for secure sandbox browser opens and OAuth loopback callbacks
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
- `zenity` or `kdialog` (for auth proxy allow/deny prompts; auto-denied if neither is available)

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
- `~/.config/ags/tmux.conf`
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
- Ensure Pi guard/settings assets exist in the host path mounted to `/home/dev/.pi`
- Optionally prompt to store configured secrets via `secret-tool`

### 4) Build/update sandbox image and agent installs

```bash
cargo run -p ags -- update
cargo run -p ags -- update-agents
```

### 5) Verify

```bash
cargo run -p ags -- doctor
cargo run -p ags -- --agent shell -- -lc 'br --version && bv --version && tmux -V && test -f ~/.tmux.conf'
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

Run inside a tmux session (opt-in):

```bash
ags --agent pi --tmux
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

### tmux inside the sandbox

The sandbox image includes `tmux` with a minimal version-controlled config for team-pane workflows.
Defaults include:

- default prefix remains `Ctrl-b`
- mouse support enabled
- pane/window creation starts in the current pane directory
- `xterm-kitty` terminal support via `kitty-terminfo`
- no plugin manager dependency

If you want the agent itself to start inside tmux, pass `--tmux`:

```bash
ags --agent pi --tmux
ags --agent shell --tmux
```

This is intentionally **not** the default.
If `tmux` is reported as missing when using `--tmux`, rebuild the sandbox image first:

```bash
ags update
```

Quick check:

```bash
ags --agent shell -- -lc 'tmux -V && test -f ~/.tmux.conf && tmux new-session -d -s smoke && tmux kill-session -t smoke'
```

Use a non-default config file:

```bash
ags --agent pi --config /path/to/config.toml
```

### MCP auth persistence

MCP servers that use OAuth (e.g. Linear) store auth tokens in `~/.mcp-auth` inside the container. Without a persistent mount, tokens are lost on every session restart. Add this to your config:

```toml
[[mount]]
host = "$HOME/.mcp-auth"
container = "/home/dev/.mcp-auth"
mode = "rw"
kind = "dir"
create = true
```

To skip the allow/deny dialog for known OAuth providers:

```toml
[auth_proxy]
auto_allow_domains = ["mcp.linear.app"]
```

### Host service access from inside sandbox

`ags` runs agent CLIs **inside the container**, so `localhost` refers to the container itself.

To connect to services running on your host machine, use `host.containers.internal` instead.

`ags` also exports runtime hints inside the container:

- `AGS_HOST_SERVICES_HOST=host.containers.internal`
- `AGS_HOST_SERVICES_HINT` (human-readable reminder)

And for agents with prompt hooks (`pi`, `claude`, `codex`), `ags` injects a short startup hint into the agent prompt context.

Example:

```bash
ags --agent shell -- -lc 'curl http://host.containers.internal:3000/health'
```

Postgres example (`psql` is available in the sandbox image after `ags update`):

```bash
ags --agent shell -- -lc 'PGPASSWORD="${PGPASSWORD:-postgres}" psql -h "${AGS_HOST_SERVICES_HOST}" -p "${PGPORT:-5432}" -U "${PGUSER:-postgres}" "${PGDATABASE:-postgres}"'
```

Tip: add `PGPASSWORD`, `PGUSER`, `PGDATABASE`, and `PGPORT` to `[sandbox].passthrough_env` if you want host values to flow into the container automatically.

---

## Commands reference

### Core commands

- `ags setup` — generate keys, ensure Pi assets in mounted host path, optional keyring secret setup
- `ags doctor` — run environment + config health checks
- `ags update` — rebuild container image from `Containerfile` and refresh bundled `br`/`bv` binaries
- `ags update-agents` — install/update agent CLIs in persistent volumes
- `ags install [--link-self] [--force] [--add-agent-mounts]` — install assets/config layout, optional self-link, optional config mount block append
- `ags uninstall` — currently reserved/no-op cleanup
- `ags create-aliases` — create managed wrappers and/or shell alias blocks
- `ags completions --shell <bash|zsh|fish>` — print shell completion script

### `create-aliases` options

```bash
ags create-aliases --mode wrappers|aliases|both --shell fish|zsh|bash --force
```

- default mode: `wrappers`
- if `--shell` omitted, shell is autodetected from `$SHELL`

### Shell completions

```bash
# bash
ags completions --shell bash > ~/.local/share/bash-completion/completions/ags

# zsh
ags completions --shell zsh > ~/.zfunc/_ags

# fish
ags completions --shell fish > ~/.config/fish/completions/ags.fish
```

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
- `[[agent_mount]]`
  - Dedicated required mounts for agent home-state paths (explicit, no implicit agent mounts)
- `[[mount]]`
  - Additional bind mounts from host to container
  - supports `mode`, `kind`, `when`, `create`, `optional`
- `[[tool]]`
  - Tool binary mount
  - optional nested `[[tool.directory]]` mounts
  - optional nested `[[tool.secret]]` sources
- `[[secret]]`
  - Map env var names to source(s): `from_env` and/or `secret_store`
- `[auth_proxy]`
  - `auto_allow_domains`: list of domains to skip the allow/deny prompt for (e.g. `["mcp.linear.app"]`)
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
- npm/pnpm lifecycle scripts are disabled in the sandbox (`ignore-scripts=true`).
- Rotate/revoke credentials quickly if compromise is suspected.
- The auth proxy requires explicit user approval (via desktop dialog) before opening any URL requested by the sandbox agent. URLs are never opened silently.
- OAuth loopback callbacks are relayed through the host proxy — the container never listens on host network ports directly.

---

## Project layout

- `crates/ags/` — Rust CLI implementation
- `config/Containerfile` — base sandbox image definition
- `config/tmux.conf` — minimal tmux defaults copied into the sandbox image
- `config/config.example.toml` — full config template
- `agent/extensions/guard.ts` — runtime guard extension mounted for Pi
- `agent/settings.example.json` — example Pi settings template
- `agent/auth-proxy-shim` — container-side `$BROWSER` replacement for auth proxy
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
- If auth proxy prompts are auto-denied: install `zenity` or `kdialog`
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
