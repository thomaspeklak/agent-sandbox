# Commands and Runtime Behavior

This document explains what each `ags` command does and what side effects to expect.

---

## CLI summary

```bash
ags [command]
ags --agent <pi|claude|codex|gemini|opencode|shell> [--browser] [--tmux] [--psp] [--psp-keep] [--config PATH] [--add-dir PATH ...] -- [agent args...]
```

Subcommands:

- `setup`
- `doctor`
- `update`
- `update-agents`
- `install`
- `uninstall`
- `create-aliases`
- `completions`

Use `ags --help` for built-in help text.

---

## Run mode (`--agent ...`)

Example:

```bash
ags --agent pi
ags --agent claude -- --model sonnet
ags --agent pi --browser
ags --agent pi --tmux
ags --agent pi --psp
ags --agent claude -d ~/code -d ~/Downloads
```

### What happens on run

1. Load and validate config.
2. Ensure embedded assets exist on disk (`Containerfile`, `tmux.conf`, guard extension).
3. Resolve secrets from configured sources.
4. Ensure sandbox git config exists.
5. Ensure dedicated SSH agent is running and keys are loaded.
6. Optionally start browser sidecar (`--browser`).
7. Start auth proxy (Unix socket + shim in per-run temp dir).
7b. Optionally start PSP sidecar (`--psp`).
8. Build launch plan (mounts/env/security/network/entrypoint).
9. Ensure image exists (builds if missing), then run `podman run`.

### Notes

- Args after `--` are passed directly to agent CLI.
- `--add-dir <path>` / `-d <path>` adds an extra same-path directory mount for the current run only; repeat it to add multiple directories.
- Container runs with rootless user namespace (`keep-id`), dropped capabilities, and `no-new-privileges`.
- Agent host state comes from explicit `[[agent_mount]]` / `[[mount]]` entries.
- Agent processes run inside the container: `localhost` is container-local. Use `host.containers.internal` for host machine ports/services.
- Runtime env vars are injected for discoverability: `AGS_HOST_SERVICES_HOST` and `AGS_HOST_SERVICES_HINT`.
- `pi`/`claude`/`codex` runs also inject a short host-service hint into prompt context.
- Interactive launches print a one-line host-service reminder before the agent CLI starts.
- `--tmux` wraps the agent command in a tmux session inside the container; this is opt-in and does not change the default launch behavior.
- `--psp` enables podman-socket-proxy mode. AGS spawns a `psp` sidecar process with a per-run Unix socket, waits for it to be ready, then mounts the socket into the container and sets `DOCKER_HOST` so Docker/Testcontainers clients route through PSP. On exit, AGS sends SIGTERM to allow PSP to clean up any containers it created, then falls back to SIGKILL after 5 seconds. PSP enforces policy-gated access to the host Podman API (deny-by-default, image allowlists, bind mount restrictions). The `psp` binary must be on `PATH` or configured via `[psp].binary` in `config.toml`. PSP picks up its own policy files (global `~/.config/psp/config.json` and project-local `.psp.json`). A stable session identifier (`PSP_SESSION_ID`) is injected into the container environment for tools that support the `x-psp-session-id` header.
- `--psp-keep` tells PSP to retain containers it created when the session ends (sets `PSP_KEEP_ON_FAILURE=true`). Useful for debugging failed test runs. Stale containers will be cleaned up automatically on the next PSP start (startup sweep).
- The auth proxy starts automatically on every run. Inside the container, `$BROWSER` points to the auth-proxy-shim. When agent code opens a URL (e.g. OAuth login), the shim sends it to the host proxy over a Unix socket. The host prompts the user via a zenity/kdialog dialog; if allowed, the URL opens in the host browser. For OAuth flows with a `localhost` callback, the host proxy captures the browser redirect and relays it back into the container. If neither `zenity` nor `kdialog` is installed, all URL-open requests are auto-denied. The proxy shuts down and cleans up its temp directory when the container exits. Domains listed in `[auth_proxy].auto_allow_domains` skip the dialog.
- Postgres quick-connect from host into sandbox shell:
  - `ags --agent shell -- -lc 'PGPASSWORD="${PGPASSWORD:-postgres}" psql -h "${AGS_HOST_SERVICES_HOST}" -p "${PGPORT:-5432}" -U "${PGUSER:-postgres}" "${PGDATABASE:-postgres}"'`

---

## `ags setup`

Initial bootstrap.

### What it does

- Generates missing SSH keys:
  - auth key
  - signing key
- Prints public keys (for GitHub SSH + signing setup).
- Ensures Pi guard/settings assets exist in the host path mounted to `/home/dev/.pi`.
- If `secret-tool` exists, prompts for optional interactive secret storage.

### Typical usage

```bash
ags setup
```

---

## `ags doctor`

Health checks for your environment and config.

### Checks include

- Required/optional host tooling
- Required config/assets presence
- Tool binaries and configured mounts
- Image presence
- SSH keys and dedicated ssh-agent state
- Secret source availability
- Session directory/writeability checks
- Browser setup checks (if enabled)

### Typical usage

```bash
ags doctor
```

---

## `ags update`

Rebuilds sandbox image from configured `Containerfile` and refreshes bundled Beads tools:

- `br` from `beads_rust` releases
- `bv` from `beads_viewer` releases

```bash
ags update
```

- Resolves latest release tags from GitHub and passes them into the image build
- Verifies release checksums during image build
- Does **not** update agent CLIs installed in persistent volumes

Version check (inside sandbox):

```bash
ags --agent shell -- -lc 'br --version && bv --version'
```

Use `ags update-agents` next if needed.

---

## `ags update-agents`

Installs/updates agent CLIs in persistent volumes using a temporary container.

```bash
ags update-agents
```

### What it updates

- Pi package (`pi_spec`)
- Codex (`@openai/codex`)
- Gemini (`@google/gemini-cli`)
- Opencode (`opencode-ai`)
- Claude install/update in dedicated volume

Settings come from `[update]` in config.

Security hardening:

- pnpm installs run with `ignore-scripts=true`.

---

## `ags install`

Installs baseline assets and optional `ags` self-link.

```bash
ags install
ags install --link-self
ags install --link-self --force
```

### What it writes

- `~/.config/ags/Containerfile`
- `~/.config/ags/tmux.conf`
- `<agent-dir>/extensions/guard.ts`
- `<agent-dir>/settings.json` (if missing)

By default `<agent-dir>` is `~/.config/ags/pi`.
It can be overridden with `AGS_AGENT_DIR`.

### Flags

- `--link-self` : create `~/.local/bin/ags` symlink to current executable
- `--force` : replace existing link/file where applicable
- `--add-agent-mounts` : append default required `[[agent_mount]]` entries to `~/.config/ags/config.toml`

---

## `ags uninstall`

Currently a reserved/no-op command.

```bash
ags uninstall
```

---

## `ags create-aliases`

Generates managed wrappers and/or shell alias blocks.

```bash
ags create-aliases
ags create-aliases --mode both --shell fish
ags create-aliases --mode wrappers --force
```

### Flags

- `--mode wrappers|aliases|both` (default: `wrappers`)
- `--shell fish|zsh|bash` (autodetect if omitted)
- `--force` (replace existing non-managed targets)

### Behavior

- Wrappers go to `~/.local/bin/`.
- Alias blocks are inserted/updated in shell rc files:
  - fish: `~/.config/fish/config.fish`
  - zsh: `~/.zshrc`
  - bash: `~/.bashrc`

Managed alias blocks are clearly delimited so future runs can update them safely.

---

## `ags completions`

Prints shell completion scripts to stdout.

```bash
ags completions --shell bash
ags completions --shell zsh
ags completions --shell fish
```

### Typical install paths

```bash
# bash
ags completions --shell bash > ~/.local/share/bash-completion/completions/ags

# zsh
ags completions --shell zsh > ~/.zfunc/_ags

# fish
ags completions --shell fish > ~/.config/fish/completions/ags.fish
```

---

## Makefile shortcuts

Equivalent convenience targets:

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
