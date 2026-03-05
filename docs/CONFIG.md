# Configuration Reference (`~/.config/ags/config.toml`)

This document explains the `ags` config schema, field by field.

Use `config/config.example.toml` as your starting template.

---

## Where config lives

Default path:

- `~/.config/ags/config.toml`

You can override at runtime:

```bash
ags --agent pi --config /path/to/config.toml
```

If the default config path does not exist, `ags` creates a minimal default file on first run.

---

## Path and env expansion behavior

For path-like fields, `ags` supports:

- `~` expansion (home directory)
- environment variable expansion:
  - `$VAR`
  - `${VAR}`

Then paths are resolved to absolute paths.

If an env var is undefined during expansion, the reference is left as-is.

---

## Top-level sections

- `[sandbox]` (required)
- `[[mount]]` (optional, repeatable)
- `[[tool]]` (optional, repeatable)
- `[[secret]]` (optional, repeatable)
- `[browser]` (optional)
- `[update]` (optional)

---

## `[sandbox]`

Core runtime settings.

```toml
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "~/.config/ags/Containerfile"
sandbox_pi_dir = "~/.config/ags/pi"
host_pi_dir = "~/.pi/agent"
host_claude_dir = "~/.claude"
agent_sandbox_base = "~/.config/ags"
cache_dir = "~/.cache/ags"
gitconfig_path = "~/.config/ags/gitconfig-agent"
auth_key = "~/.ssh/ags-agent-auth"
sign_key = "~/.ssh/ags-agent-signing"
bootstrap_files = ["auth.json", "models.json"]
container_boot_dirs = ["/home/dev/.ssh"]
passthrough_env = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY"]
```

### Fields

- `image` (string, required)
  - Podman image tag used for runs.
- `containerfile` (path, required)
  - Containerfile path used by `ags update` and auto-build fallback.
- `sandbox_pi_dir` (path, required)
  - Currently part of schema; per-agent launch directories derive from `agent_sandbox_base`.
- `host_pi_dir` (path, required)
  - Host Pi directory used during setup bootstrap.
- `host_claude_dir` (path, required)
  - Host Claude directory used for mounts and setup bootstrap.
- `agent_sandbox_base` (path, optional, default `~/.config/ags`)
  - Base directory for per-agent state (`<base>/pi`, `<base>/codex`, etc).
- `cache_dir` (path, required)
  - Host cache dir for ssh-agent env/socket and tool caches.
- `gitconfig_path` (path, required)
  - Host path for generated git signing config used in container.
- `auth_key` (path, required)
  - SSH key for git auth.
- `sign_key` (path, required)
  - SSH key for commit signing.
- `bootstrap_files` (string array, optional)
  - Reserved bootstrap file list.
- `container_boot_dirs` (string array, optional)
  - Directories created in container before launching agent.
- `passthrough_env` (string array, optional)
  - Host env vars to pass into container if set and not already resolved from secrets.

---

## `[[mount]]`

Extra host bind mounts.

```toml
[[mount]]
host = "~/.ssh/known_hosts"
container = "/home/dev/.ssh/known_hosts"
mode = "ro"
kind = "file"
when = "always"
create = false
optional = true
source = "config"
```

### Fields

- `host` (path, required)
- `container` (string, required)
- `mode` (`"ro" | "rw"`, required)
- `kind` (`"dir" | "file"`, optional, default `"dir"`)
- `when` (`"always" | "browser"`, optional, default `"always"`)
- `create` (bool, optional, default `false`)
  - If host path missing, create it automatically.
- `optional` (bool, optional, default `false`)
  - If host path missing and `create=false`, skip instead of failing.
- `source` (string, optional, default `"config"`)
  - Label used in diagnostics/errors.

### Missing-path behavior

If host path is missing:

- `create=true` → path is created
- else if `optional=true` → mount is skipped
- else → run fails (`required mount source missing`)

---

## `[[tool]]`

Declares a tool binary mount, optional directories, optional secrets.

```toml
[[tool]]
name = "qwk"
path = "~/.local/bin/qwk"
container_path = "/usr/local/bin/qwk"
mode = "ro"
when = "always"
optional = true

[[tool.directory]]
host = "~/.config/qwk"
container = "/home/dev/.config/qwk"
mode = "rw"
kind = "dir"
create = true

[[tool.secret]]
env = "QWK_LINEAR_API_KEY"
from_env = "QWK_LINEAR_API_KEY"
```

### `[[tool]]` fields

- `name` (string, required)
- `path` (path, required)
- `container_path` (string, required)
- `mode` (`ro|rw`, optional, default `ro`)
- `when` (`always|browser`, optional, default `always`)
- `optional` (bool, optional)

### `[[tool.directory]]`

Same schema/behavior as `[[mount]]`.

### `[[tool.secret]]`

Same schema as `[[secret]]`, but tagged to that tool for diagnostics.

---

## `[[secret]]`

Maps target env var names to one or more sources.

```toml
[[secret]]
env = "GH_TOKEN"
from_env = "GH_TOKEN"

[[secret]]
env = "GH_TOKEN"
secret_store = { service = "github-cli-login-switcher", username = "general" }
```

### Recommended modern fields

- `env` (required): target env var name inside container
- `from_env` (optional): source env var from host process environment
- `secret_store` (optional): key/value attributes for `secret-tool lookup`

A single entry can include one or both source types.

### Legacy fields (still accepted)

- `provider = "env" | "secret-tool"`
- `var` (for env provider)
- `attributes` (for secret-tool provider)

### Resolution behavior

- Secrets are processed in config order.
- For the same target `env`, first successful source wins.
- Empty/unresolved sources are ignored.

---

## `[browser]`

Controls optional browser sidecar used with `--browser`.

```toml
[browser]
enabled = true
command = "google-chrome"
profile_dir = "~/.cache/ags/chrome-profile"
debug_port = 9222
pi_skill_path = "/home/dev/browser-tools"
command_args = []
```

### Fields

- `enabled` (bool, default `false`)
- `command` (string)
  - Required when enabled.
  - Can be a PATH command (`google-chrome`) or executable path.
- `profile_dir` (path)
  - Required when enabled.
- `debug_port` (u16)
  - Required and non-zero when enabled.
- `pi_skill_path` (string)
  - Injected for Pi runs in browser mode (`--skill <path>`).
- `command_args` (string array)
  - Extra args passed to browser command.

---

## `[update]`

Controls `ags update-agents` behavior.

```toml
[update]
pi_spec = "@mariozechner/pi-coding-agent"
minimum_release_age = 1440
```

### Fields

- `pi_spec` (string, default `@mariozechner/pi-coding-agent`)
  - Package spec used for Pi install/update.
- `minimum_release_age` (u32, default `1440`)
  - Written to pnpm config (`minimum-release-age`) inside update container.

---

## Validation tips

- Run `ags doctor` after config changes.
- Keep required mounts minimal and explicit.
- Prefer `optional=true` for machine-specific paths.
- Prefer `mode="ro"` unless writes are necessary.
- Keep browser section disabled unless you actively use it.
