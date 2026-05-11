# Configuration Reference (`~/.config/ags/config.toml` + optional `PROJECT_ROOT/.ags/config.toml`)

This document explains the `ags` config schema, field by field.

Use `config/config.example.toml` as your starting template.

---

## Where config lives

Base config path:

- `~/.config/ags/config.toml`

Optional repo-local overlay path:

- `PROJECT_ROOT/.ags/config.toml`

You can override the base config at runtime:

```bash
ags --agent pi --config /path/to/config.toml
```

If the base config path does not exist, `ags` creates a minimal default file on first run.

### Precedence and merge behavior

When AGS is launched inside a git repo, it resolves `PROJECT_ROOT` from the active repository/worktree root (`git rev-parse --show-toplevel`) and then loads config in this order:

1. base config (`~/.config/ags/config.toml`, or `--config <path>` if provided)
2. repo-local overlay (`PROJECT_ROOT/.ags/config.toml`) if present

Merge rules:

- scalar fields: repo-local value overrides base value
- table/object fields: merged recursively; repo-local keys win
- repeatable top-level sections are additive:
  - `[[mount]]`
  - `[[agent_mount]]`
  - `[[tool]]`
  - `[[secret]]`
- other arrays are replaced by the repo-local value

This lets a project add mounts/tools/secrets locally without copying your full personal config.

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
- `[[agent_mount]]` (optional, repeatable, recommended)
- `[[mount]]` (optional, repeatable)
- `[[tool]]` (optional, repeatable)
- `[[secret]]` (optional, repeatable)
- `[browser]` (optional)
- `[host_ui]` (optional)
- `[update]` (optional)

---

## `[sandbox]`

Core runtime settings.

```toml
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "~/.config/ags/Containerfile"
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
  - Containerfile path used by `ags update-image` and auto-build fallback.
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

## `[[agent_mount]]`

Dedicated, explicit mounts for agent home-state paths (no implicit runtime mounts).

```toml
[[agent_mount]]
host = "~/.pi"
container = "/home/dev/.pi"

[[agent_mount]]
host = "~/.claude"
container = "/home/dev/.claude"

[[agent_mount]]
host = "~/.claude.json"
container = "/home/dev/.claude.json"
kind = "file"
```

### Fields

- `host` (path, required)
- `container` (string, required)
- `kind` (`"dir" | "file"`, optional, default `"dir"`)

Behavior:

- mode is always `rw`
- `when` is always `always`
- mount is always required (`optional=false`, `create=false`)

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

### Recommended optional mounts for dcg

If you want sandboxed `dcg` to use host-managed global config and persist its own user-level state, add mounts like:

```toml
[[mount]]
host = "$HOME/.config/dcg"
container = "/home/dev/.config/dcg"
mode = "rw"
kind = "dir"
optional = true

[[mount]]
host = "$HOME/.local/share/dcg"
container = "/home/dev/.local/share/dcg"
mode = "rw"
kind = "dir"
optional = true
create = true
```

Notes:

- project-local `.dcg.toml` is picked up automatically from the workspace mount
- `~/.config/dcg` covers global config, allowlists, and allow-once state
- `~/.local/share/dcg` covers dcg history storage
- if you do not mount these, sandbox dcg still works with built-in defaults plus project-local config

Recommended host-side `dcg` starting point:

```toml
# ~/.config/dcg/config.toml
[packs]
enabled = [
  "database.postgresql",
  "containers.docker",
]
```

This keeps dcg core protections on (implicit) and adds common AGS-adjacent packs without inventing AGS-specific policy syntax.

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

## `[host_ui]`

Controls optional host-owned Glimpse windows for sandboxed code.

```toml
[host_ui]
enabled = true
binary = "/path/to/glimpse_host_ui"
renderer = "process"
renderer_bin = "/path/to/glimpse"
idle_timeout_ms = 0
log_level = "info"
```

### Fields

- `enabled` (bool, default `false`)
  - Starts the AGS host UI sidecar for each run.
- `binary` (string, default `glimpse-host-ui`)
  - Path or command for the host UI service.
- `renderer` (string, default `stub`)
  - Renderer backend name passed to the host UI service.
  - Use `process` for normal interactive Glimpse windows.
  - `stub` is mainly useful for testing.
- `renderer_bin` (path, optional)
  - Required when the chosen renderer needs an external binary, such as `renderer = "process"`.
- `idle_timeout_ms` (u64, default `0`)
  - Idle timeout passed to the host UI service.
  - `0` disables idle shutdown so the sidecar stays alive for the AGS session.
- `log_level` (string, default `info`)
  - Logging level passed to the host UI service.

Notes:

- AGS handles the sandbox wiring automatically once `[host_ui].enabled = true`.
- Users normally should not set Glimpse transport env vars manually.
- For end-user setup and troubleshooting, see `docs/GLIMPSE.md`.

---

## `[update]`

Controls `ags update-agents` behavior.

```toml
[update]
pi_spec = "@earendil-works/pi-coding-agent"
minimum_release_age = 1440
```

### Fields

- `pi_spec` (string, default `@earendil-works/pi-coding-agent`)
  - Package spec used for Pi install/update.
  - Older configs with the exact legacy value `@mariozechner/pi-coding-agent` should be updated; `ags update-agents` treats that value as the current default during migration.
- `minimum_release_age` (u32, default `1440`)
  - Written to pnpm config (`minimum-release-age`) inside update container.

---

## Validation tips

- Run `ags doctor` after config changes.
- Keep required mounts minimal and explicit.
- Prefer `optional=true` for machine-specific paths.
- Prefer `mode="ro"` unless writes are necessary.
- Keep browser section disabled unless you actively use it.
