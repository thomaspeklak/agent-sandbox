# pi-sandbox

Hardened `pi` launcher using rootless Podman.

## Security-first note (required)

**Only use limited tokens in sandbox runs.**

- least-privilege scopes only
- dedicated bot/machine credentials preferred
- short expiration where possible
- rotate regularly
- revoke immediately on suspicion

Do **not** inject broad personal tokens.

## Install

From the project root:

```bash
make install
# or: ./scripts/install.sh
```

This creates symlinks for:

- `~/.local/bin/pis`
- `~/.local/bin/pisb`
- `~/.local/bin/pis-setup`
- `~/.local/bin/pis-doctor`
- `~/.local/bin/pis-update`
- `~/.config/pi-sandbox` -> `<project>/config`
- `~/.pi/agent-sandbox/extensions/guard.ts` -> `<project>/agent/extensions/guard.ts`

And for settings:

- `~/.pi/agent-sandbox/settings.json` is **copied** (not symlinked)
  - first choice: `~/.pi/agent/settings.json`
  - fallback: `<project>/agent/settings.example.json`

Legacy aliases are removed on install (`pi-sbox*`).

## Commands

- `pis` - sandbox mode
- `pisb` - browser-enabled mode
- `pis setup` / `pis-setup` - generate SSH keys + optional secret-store population
- `pis doctor` / `pis-doctor` - diagnostics from configured integrations
- `pis update` / `pis-update` - rebuild image and refresh final pi-install layer
- `pis install` / `pis uninstall` - manage symlink installation

## Config-driven integrations

All mounts, tools, and secret sources come from:

- `~/.config/pi-sandbox/config.toml`

No integration is auto-detected by hardcoded tool defaults.

Built-in language/tool caches are persisted under `sandbox.cache_dir`:
- pnpm (`pnpm-home`)
- cargo (`cargo-home`)
- go (`go-path`, `go-build`)
- rust compiler cache tools (`sccache`, `cachepot`)

Use `config/config.example.toml` as the schema/template. It contains dummy placeholders only.

### Tool + directory + secret pattern

```toml
[[tool]]
name = "example-tool"
path = "~/path/to/example-tool"
container_path = "/usr/local/bin/example-tool"
mode = "ro"
optional = true

[[tool.directory]]
host = "~/.config/example-tool"
container = "/home/dev/.config/example-tool"
mode = "rw"
kind = "dir"
create = true

[[tool.secret]]
env = "EXAMPLE_TOOL_TOKEN"
from_env = "EXAMPLE_TOOL_TOKEN"

[[tool.secret]]
env = "EXAMPLE_TOOL_TOKEN"
secret_store = { service = "example-service", username = "example-user" }
```

### Secret source pattern

```toml
[[secret]]
env = "EXAMPLE_GH_TOKEN"
from_env = "EXAMPLE_GH_TOKEN"

[[secret]]
env = "EXAMPLE_GH_TOKEN"
secret_store = { service = "example-gh-service", account = "example-account" }
```

Resolution order is declaration order in `config.toml` (first successful source wins per env var).

## One-time setup

```bash
pis setup
```

Then upload generated keys:

- `~/.ssh/pi-agent-auth.pub` as GitHub SSH auth key
- `~/.ssh/pi-agent-signing.pub` as GitHub SSH signing key

## Usage

```bash
cd /path/to/repo
pis
pis --continue
pisb
pis doctor
```

## Update pi in image

```bash
pis update
```

Advanced:

```bash
pis update --pi-spec @mariozechner/pi-coding-agent@0.55.1
pis update --minimum-release-age 2880
```
