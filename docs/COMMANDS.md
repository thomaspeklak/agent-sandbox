# Commands and Runtime Behavior

This document explains what each `ags` command does and what side effects to expect.

---

## CLI summary

```bash
ags [command]
ags --agent <pi|claude|codex|gemini|opencode|shell> [--browser] [--config PATH] -- [agent args...]
```

Subcommands:

- `setup`
- `doctor`
- `update`
- `update-agents`
- `install`
- `uninstall`
- `create-aliases`

Use `ags --help` for built-in help text.

---

## Run mode (`--agent ...`)

Example:

```bash
ags --agent pi
ags --agent claude -- --model sonnet
ags --agent pi --browser
```

### What happens on run

1. Load and validate config.
2. Ensure embedded assets exist on disk (`Containerfile`, guard extension).
3. Resolve secrets from configured sources.
4. Ensure sandbox git config exists.
5. Ensure dedicated SSH agent is running and keys are loaded.
6. Optionally start browser sidecar (`--browser`).
7. Build launch plan (mounts/env/security/network/entrypoint).
8. Ensure image exists (builds if missing), then run `podman run`.

### Notes

- Args after `--` are passed directly to agent CLI.
- Container runs with rootless user namespace (`keep-id`), dropped capabilities, and `no-new-privileges`.
- Per-agent host state is persisted under your configured sandbox base/cache paths.

---

## `ags setup`

Initial bootstrap.

### What it does

- Generates missing SSH keys:
  - auth key
  - signing key
- Prints public keys (for GitHub SSH + signing setup).
- Bootstraps per-agent sandbox directories from host config when available.
- Writes Pi guard/settings assets into Pi sandbox.
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

Rebuilds sandbox image from configured `Containerfile`.

```bash
ags update
```

- Rebuilds dependencies/image layer
- Does **not** update agent CLIs installed in persistent volumes

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
- `<agent-dir>/extensions/guard.ts`
- `<agent-dir>/settings.json` (if missing)

By default `<agent-dir>` is `~/.config/ags/pi`.
It can be overridden with `AGS_AGENT_DIR`.

### Flags

- `--link-self` : create `~/.local/bin/ags` symlink to current executable
- `--force` : replace existing link/file where applicable

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
