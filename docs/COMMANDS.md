# Commands and Runtime Behavior

This document explains what each `ags` command does and what side effects to expect.

---

## CLI summary

```bash
ags [command]
ags --agent <pi|claude|codex|gemini|opencode|shell> [--browser] [--tmux] [--stop-when-done] [--psp] [--psp-keep] [--yolo] [--root] [--lockdown] [--wayland-compositor-passthrough] [--defaults|-D] [--config PATH] [--add-dir PATH ...] [--env NAME=VALUE ...] -- [agent args...]
ags node <install VERSION|list> [--config PATH]
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
- `tools`
- `node install <version>` / `node list` (the `runtime` and `runtimes` names are accepted aliases)

Use `ags --help` for built-in help text.

---

## `ags tools`

Profession-guided sandbox tool chooser.

Typical usage:

```bash
ags tools --packages config/tool-packages.example.json
ags tools config/tool-packages.example.json --config ~/.config/ags/config.toml
```

What it does:

- Reads a JSON catalog with canonical `agents`, `tools`, and ordered groups.
- Shows horizontal AI Tools, General, Software Development, and Operations and DevOps tabs when the catalog defines all four.
- Groups each profession's tools under area dividers such as Languages, Source control, Network, and Administration.
- Keeps one selection state when a tool appears in several professions or areas.
- Loads and edits only the selected base config (`~/.config/ags/config.toml` or `--config`); repository-local overlays do not affect this command.
- Treats omitted tool-selection fields as the catalog default tool set.
- Marks catalog defaults in the list; press `d` to restore those recommendations.
- Opens the catalog-defined Agent CLIs panel with `a`; only known, valid catalog agents are available.
- Stores selected agent IDs in `[sandbox].enabled_agents`; omitted configuration keeps all current agent CLIs enabled.
- Resolves selected GitHub-backed image tools and saves them with pinned downloads in a content-addressed `tool-downloads.<sha256>.lock.json` beside the base config.
- Saves typed policies for selected agent providers in `agent-providers.<sha256>.lock.json`, updates both lock references, and creates a config backup.
- Preserves configured package names that are not represented in the catalog, except fixed AGS baseline packages that no longer belong in the extra list.
- Preserves unknown locked download tools until a catalog explicitly manages their IDs or a selected catalog tool owns the same installed command.
- Removes obsolete `[[tool]]` entries created by older versions of this configurator while preserving user-authored tool mounts.
- Prints image-component and agent changes, then prompts you to run `ags update-image` and `ags update-agents`.

The catalog defines each known agent with a stable `id`, display metadata, and a closed provider policy: pnpm for Pi/Gemini, a trusted built-in installer for Claude/Codex, or a GitHub release for OpenCode. It defines each purposeful executable tool once with a stable `id`, display `name`, purpose-focused `description`, required `default` flag, and exactly one installation provider. DNF tools own one or more internal `dnf_packages`. Pinned downloads declare a version, archive format, executable member, destination command, and HTTPS URL plus SHA-256 for both `x86_64` and `aarch64`. GitHub-backed tools declare a repository, latest-or-exact release policy, anchored architecture asset selectors, and optional checksum selectors. Groups contain ordered subcategories that reference tool IDs. A tool may be referenced several times, but each package or downloaded command belongs to one canonical tool. Multi-package tools, such as tmux with its terminal metadata dependency, are selected only when all owned packages are configured.

`ags tools` only edits configuration and its generated locks. It may fetch GitHub release metadata and small checksum files while resolving selected sources, but it does not invoke a host package manager, download executable archives, inspect host `PATH`, mount host binaries, or modify user-authored `[[tool]]` and `[[secret]]` entries. Executable archives are downloaded only while building the sandbox image or reconciling OpenCode, use HTTPS, and must pass the pinned SHA-256 check. The only `[[tool]]` entries the picker removes are obsolete entries marked as owned by an older version of the configurator. Libraries, headers, certificate bundles, AGS runtimes, and standard utilities such as curl are not presented as tools. Deselecting a tool prevents AGS from requesting its optional image component explicitly; another selected component may still provide the same executable as a dependency.

Agent CLIs are not installed in the base image. `ags update-agents` installs selected agents into a new persistent runtime generation, omitting deselected agents while preserving host authentication, settings, and existing sessions. Shell is always available and is not shown in the agent checklist.

---

## Run mode (`--agent ...`)

Example:

```bash
ags --agent pi
ags --agent claude -- --model sonnet
ags --agent claude --defaults -- --model opus
ags --agent pi --browser
ags --agent pi --tmux
ags --agent pi --psp
ags --agent claude --lockdown
ags --agent claude -d ~/code -d ~/Downloads
ags --agent pi --env BROWSER_URL=http://127.0.0.1:9222
```

### What happens on run

1. Load and validate config.
2. Ensure embedded assets exist on disk (`Containerfile`, `tmux.conf`, and any needed staged guard assets).
3. If not running with `--lockdown`, resolve secrets from configured host environment, keyring, or trusted command sources. Command helpers run on the host before container startup.
4. If not running with `--lockdown`, ensure sandbox git config exists.
5. If not running with `--lockdown`, ensure dedicated SSH agent is running and keys are loaded.
6. If not running with `--lockdown` and requested, start browser sidecar (`--browser`).
7. If not running with `--lockdown`, start auth proxy (Unix socket + shim in per-run temp dir).
8. If not running with `--lockdown` and `[host_ui].enabled = true`, start host UI sidecar.
9. If not running with `--lockdown` and `[clipboard].enabled = true`, start clipboard bridge sidecar and mount shims.
10. If not running with `--lockdown`, start webview origin relay for sandbox-served app origins.
11. If not running with `--lockdown` and requested, start PSP sidecar (`--psp`).
12. If running with `--lockdown`, stage a sanitized per-run agent home/runtime for the selected agent.
13. Build launch plan (mounts/env/security/network/entrypoint).
14. For Pi/Claude runs with guards enabled, verify the sandbox image contains `dcg` and warn if it does not.
15. Ensure image exists (builds if missing), then run `podman run`.
16. In ordinary (non-lockdown) runs, mount the persistent AGS mise Node store read-only and install ephemeral Node command wrappers. Each wrapper finds the nearest `.nvmrc` under the initial workspace on every invocation, including noninteractive commands and nested `cd` paths.

### Notes

- Args after `--` are passed directly to agent CLI.
- `--defaults` / `-D` prepends AGS-managed default passthrough args for the selected agent harness. Today that means Claude gets `--strict-mcp-config --dangerously-skip-permissions`, Gemini gets `--yolo`, and other agents currently add nothing.
- `--add-dir <path>` / `-d <path>` adds an extra same-path directory mount for the current run only; repeat it to add multiple directories.
- User-managed Node versions live under `<cache_dir>/mise`. `ags node install 22` and `ags node list` run mise in the Linux sandbox image; installation is the only workflow that mounts this store read-write. During normal runs it is mounted at `/opt/ags/mise:ro`. This protects the store from writes by the sandboxed agent, but it is not an integrity boundary against its host owner or another process that can write the host cache; install only versions you trust.
- `.nvmrc` selection accepts only one numeric major/minor/patch selector (for example `22`, `22.14`, or `22.14.0`; optional `v` prefix). AGS does not evaluate `.nvmrc` shell content, aliases such as `lts/*`, or any project `mise.toml`, hooks, or environment directives. A missing requested version fails with `ags node install <version>` remediation; it never falls back to Node 24. Without `.nvmrc`, `/usr/bin/node` from the fixed Node 24 image baseline is used.
- These are Linux-container runtimes, not host installations. `install` and `list` require local Podman that can run the configured Linux image; a managed runtime is usable only in an ordinary AGS run on a compatible container architecture. Existing images must be rebuilt with `ags update-image` to receive mise.
- `--lockdown` deliberately omits the store, Node wrappers, and their environment variables. This keeps the lockdown agent runtime ephemeral; it uses only the image baseline and cannot use user-managed `.nvmrc` selection.
- `--env <NAME=VALUE>` sets a container environment variable for the current run; repeat it to set multiple values. Names use shell identifier syntax, the internal `AGS_` prefix is reserved, values may be empty or contain `=`, and a later assignment to the same name wins. Explicit values override AGS-managed defaults. Because values appear in the host command line, use the secret transports instead for credentials.
- `--yolo` disables AGS-managed Pi/Claude guard integrations for that run. For Pi, the AGS guard extension sees `AGS_GUARD_YOLO=1` and becomes a no-op; for Claude, AGS omits its PreToolUse guard hook wiring.
- `--lockdown` minimizes host exposure for the current run. It disables configured secrets and passthrough env, SSH agent wiring, sandbox git config, generic `[[mount]]` entries, `[[tool]]`-derived mounts/secrets, host bridges/sidecars (including config-enabled host UI for that run), and direct mounting of the selected agent home. Instead AGS stages a sanitized ephemeral home/runtime for the selected agent and discards prior/current session history artifacts when the run exits.
- In lockdown mode, `--add-dir` still works, network access stays enabled, and exact workspace/external git metadata mounts still work as usual.
- Incompatible with `--browser`, `--psp`, `--psp-keep`, `--root`, and `--wayland-compositor-passthrough`.
- Container runs with rootless user namespace (`keep-id`), dropped capabilities, and `no-new-privileges`.
- Agent host state normally comes from explicit `[[agent_mount]]` / `[[mount]]` entries; lockdown overrides that with staged per-run agent state.
- Agent runtime and `[[agent_mount]]` resources for disabled agents are not mounted. Resources for all enabled agents retain the existing shared behavior.
- Launching an agent absent from `[sandbox].enabled_agents` fails before Podman starts; enable it through `ags tools` and run `ags update-agents`.
- Agent processes run inside the container: `localhost` is container-local. Use `host.containers.internal` for host machine ports/services.
- Outside lockdown, runtime env vars are injected for discoverability: `AGS_HOST_SERVICES_HOST` and `AGS_HOST_SERVICES_HINT`.
- `pi`/`claude`/`codex`/`opencode` runs inject a short host-service hint into prompt context, including in lockdown.
- Outside lockdown, interactive launches print a one-line host-service reminder before the agent CLI starts.
- `--tmux` wraps the agent command in a tmux session inside the container. After the agent exits, an interactive shell remains available for inspection. Combine with `--stop-when-done` to exit immediately instead.
- `--stop-when-done` (requires `--tmux`) exits the container as soon as the agent process finishes instead of dropping to an interactive shell. Useful for batch/CI runs where you don't need post-task inspection.
- `--wayland-compositor-passthrough` mounts the real host Wayland compositor socket. This is broad desktop access and is separate from clipboard support; keep it off unless you intentionally need sandbox GUI clients to connect directly to the compositor.
- The sidecar/bridge notes below apply to normal runs; lockdown suppresses them.
- `--psp` enables podman-socket-proxy mode. AGS spawns a `psp` sidecar process with a per-run Unix socket, waits for it to be ready, then mounts the socket into the container and sets `DOCKER_HOST` so Docker/Testcontainers clients route through PSP. On exit, AGS sends SIGTERM to allow PSP to clean up any containers it created, then falls back to SIGKILL after 5 seconds. PSP enforces policy-gated access to the host Podman API (deny-by-default, image allowlists, bind mount restrictions). The `psp` binary must be on `PATH` or configured via `[psp].binary` in `config.toml`. PSP picks up its own policy files (global `~/.config/psp/config.json` and project-local `.psp.json`). A stable session identifier (`PSP_SESSION_ID`) is injected into the container environment for tools that support the `x-psp-session-id` header.
- `--psp-keep` tells PSP to retain containers it created when the session ends (sets `PSP_KEEP_ON_FAILURE=true`). Useful for debugging failed test runs. Stale containers will be cleaned up automatically on the next PSP start (startup sweep).
- The auth proxy starts automatically on every run. Inside the container, `$BROWSER` points to the auth-proxy-shim, which is also exposed as the standard `xdg-open` and `sensible-browser` commands so third-party Linux tools use the same path. When agent code opens a URL (e.g. OAuth login), the shim sends it to the host proxy over a Unix socket. The host prompts the user through the shared AGS dialog renderer: a branded Glimpse host-UI dialog when `[host_ui]` is enabled, otherwise zenity/kdialog fallback. Standard URLs get **Open** / **Cancel**. If the target itself is `http://localhost:<port>/...` or `http://127.0.0.1:<port>/...` and the AGS webview relay is available, the dialog also offers **Proxy**, which rewrites the URL through the same dedicated host-port relay used for sandbox-served Glimpse apps. When AGS host UI is enabled, Proxy prefers opening that relayed URL in a host-owned Glimpse window; otherwise it falls back to the normal host browser. For OAuth flows with a `localhost` callback, the host proxy captures the browser redirect and relays it back into the container. If no dialog renderer is available, all URL-open requests are auto-denied. The proxy shuts down and cleans up its temp directory when the container exits. Domains listed in `[auth_proxy].auto_allow_domains` skip the dialog.
- If `[host_ui].enabled = true`, AGS starts a per-session host UI service and mounts `/run/ags-host-ui` into the sandbox. The host owns the actual Glimpse window; sandboxed code only sees the socket-backed client API. For user-facing setup and troubleshooting, see `docs/GLIMPSE.md`.
- If `[clipboard].enabled = true`, AGS starts a per-session clipboard bridge, mounts `/run/ags-clipboard`, shadows `wl-paste`/`wl-copy` with shims in `/home/dev/.local/bin`, and sets `XDG_SESSION_TYPE=wayland` so Pi's Ctrl-V image paste uses the shim path. Pi Ctrl-V image paste reads host clipboard through this bridge; by default the host prompts once and can allow reads for `[clipboard].approval_seconds`. `/copy`-style flows write through it when `mode = "readwrite"`.
- The webview origin relay also starts automatically. It exposes `AGS_WEBVIEW_RELAY_SOCKET` and `AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET` inside the sandbox plus a helper command `ags-webview-url <port> [base_path]`. Use it when a host-owned webview must load a temporary HTTP app server running on `127.0.0.1:<port>` inside the container. The helper returns a dedicated host origin for that app, but Glimpse-based packages should normally just pass their ordinary localhost URL to `glimpseui` and let Glimpse resolve it automatically.
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
- Whether configured `dcg` is available inside the sandbox image
- SSH keys and dedicated ssh-agent state
- Secret source availability
- Session directory/writeability checks
- Browser setup checks (if enabled)

### Typical usage

```bash
ags doctor
```

---

## `ags update-image`

Rebuilds the sandbox image from the configured `Containerfile`, including the DNF and verified-download tools selected through `ags tools`.

```bash
ags update-image
ags update-image --keep-existing
ags update-image --config /path/to/config.toml
```

- Uses the immutable architecture-specific URLs and SHA-256 values in `[sandbox].tool_download_lock`
- Supports exact executable extraction from `zip`, `tar.gz`, and `tar.xz` archives
- Verifies every selected archive during image build before installing its declared executable
- Removes the previously tagged sandbox image after the new build succeeds, unless a container still references it
- Referenced previous images are retained with a warning listing the blocking container IDs
- `--keep-existing` keeps the previous image for manual rollback/debugging
- `--config` selects the base config file used for the build; a trusted repo-local overlay still takes precedence
- Does **not** update agent CLIs installed in persistent volumes

`ags update` remains as a deprecated alias for `ags update-image`.

Version check (inside sandbox):

```bash
ags --agent shell -- -lc 'br --version && bv --version && dcg --version'
```

Use `ags update-agents` next if needed.

---

## `ags node`

Manage user-installed Node.js versions through mise in a throwaway Linux helper container:

```bash
ags node install 22
ags node install 22.14.0
ags node list
```

The store persists below `[sandbox].cache_dir` (default `~/.cache/ags/mise`). `install` uses a network-enabled helper with a read-write store mount; `list` uses a network-disabled helper with a read-only store mount. The sandbox image supplies the fixed Node 24 baseline and the Fedora `mise` package. The store is shared by ordinary AGS runs, so its read-only mount prevents sandbox writes but does not protect against a host process that can write the cache. Rebuild an existing image with `ags update-image` if it predates mise. `--lockdown` does not expose this store or `.nvmrc` selection.

## `ags update-agents`

Builds and verifies an isolated runtime candidate using temporary containers. It publishes a new immutable generation only when the verified runtime identity changes.

```bash
ags update-agents
ags update-agents --config /path/to/config.toml
```

### What it reconciles

- Installs or updates enabled Pi, Codex, Gemini, OpenCode, and Claude runtimes.
- Omits disabled agents from the new generation without removing files used by existing sessions.
- Preserves agent authentication and settings under the configured `[[agent_mount]]` host paths.
- Uses an installer-only pnpm download store at `<cache_dir>/agent-downloads/pnpm-store`, rather than retaining a copy in every generation. Imports use copy-on-write clones or copies, never hard links to the writable store.

### No-op updates and disk sharing

Each verified generation records a runtime manifest: exact sandbox image ID, enabled agents/providers, runtime file hashes, permissions, symlinks, and installed pnpm dependency contents. pnpm's random installation-directory IDs and generated bookkeeping timestamps do not change runtime identity. A dependency-only change still counts as an update, even if the agent's own version is unchanged.

If the candidate matches the intact selected generation, AGS reports **Already up to date**, discards the candidate, and leaves both `current` and `previous` unchanged. Cleanup still runs. Existing generations without a manifest are rebuilt once. The image is resolved to an immutable ID and used for both installation and verification.

**Currently this is a verified-candidate comparison, not a registry-only preflight:** installers still run to resolve eligible versions and dependencies; temporary disk space and installer/download work may still be needed. This avoids assuming that matching top-level version numbers imply identical dependencies or binaries.

For changed candidates, identical regular files are hard-linked from the intact selected generation **only after installation and verification containers have exited**. Exact file hashes and permissions must match. No installer receives a tree linked to a running generation; legacy installs and the writable package-manager store are never sharing sources. Published mounts remain read-only. Filesystems that cannot hard-link retain independent copies. Deleting an old generation merely drops its links; retained generations keep their files. Do not modify published runtime files manually: shared files are immutable by design.

### Development pnpm storage

Normal sandboxes—including shell-only, Claude-only, and OpenCode-only configurations—receive writable pnpm store and metadata-cache mounts scoped to the canonical Git worktree:

```text
<cache_dir>/workspace-caches/<worktree-identity>/pnpm-store
<cache_dir>/workspace-caches/<worktree-identity>/pnpm-cache
        → /var/cache/ags/pnpm/{store,cache}
```

The identity includes the canonical worktree path and its Git metadata inode. Subdirectories of one worktree share a cache; separate worktrees do not. Recreating a checkout at the same path creates a fresh identity. Only the two cache subdirectories enter the sandbox; coordination metadata and other worktrees remain outside its mounts.

`PNPM_HOME` and pnpm's global bin directory use the separate, container-local `/home/dev/.local/share/pnpm-user`. The managed agent paths remain ahead of it on `PATH`, and AGS launches managed agents through explicit generation paths. Project virtual stores remain inside each project, imports use `clone-or-copy`, integrity verification is enabled, and the side-effects cache is disabled. Lockdown sessions mount no persistent development cache and use `/tmp/ags-pnpm/*` instead.

Existing project `node_modules` trees may record the former `/usr/local/pnpm/.store` location and produce `ERR_PNPM_UNEXPECTED_STORE`. When the worktree is idle, reinstall its dependencies; AGS does not rewrite pnpm metadata or delete `node_modules` during launch. Rebuild the sandbox image with `ags update-image` to pick up the pinned pnpm version and matching defaults.

Development caches and updater caches are separate trust domains: neither seeds the other, and no hard links are created across those boundaries. Published generations remain usable if either cache is deleted or unavailable. AGS rejects a writable workdir or configured mount whose host path is an ancestor of `agent-runtimes`, `agent-downloads`, or `workspace-caches`; otherwise the same protected files could be reached through a writable alias despite their dedicated mount policy.

### Running-session safety

Generations live under `<cache_dir>/agent-runtimes/generation-*`. The `current` file is an atomically replaced selection, not a container mount. Each new sandbox resolves it once and mounts concrete generation directories read-only. Existing sandboxes, including new agent processes started inside them, keep their original runtime.

Before installation, `update-agents` runs `podman ps --all --quiet --no-trunc` and `podman container inspect` for each container. It reports runtime generations referenced by actual mount sources and container status, including stopped containers that can be restarted. Legacy runtime mounts are reported too. Inspection errors abort the update rather than treating unknown usage as unused.

An OS file lock serializes updates for the same cache. Installation happens in a fresh generation; a second container checks each enabled CLI with a bounded `--version` smoke test against read-only runtime mounts. Only successful verification and a changed runtime manifest publish the selection. Failed/interrupted builds remain unselected, and do not advance the previous-generation pointer. Authentication and session data are not copied into generations.

**Automatic cleanup:** after a successful update or no-op check, AGS retains the latest generation, the previous successfully published generation, and every generation referenced by a running or stopped container. Other completed generations are deleted. Container references are inspected again immediately before cleanup; the pre-install snapshot is not reused. Inspection failures skip cleanup and emit a warning without undoing the successful update.

Pending launches hold a shared lease on their selected generation, so even a launch delayed across several updates is protected. Cleanup takes a short selection lock and only deletes generations whose lease it can lock exclusively. These leases do not block publication or cleanup of other generations. A launch-plan lease normally lasts until its Podman command exits; container inspection protects detached or stopped containers afterward.

**Legacy cleanup:** the old `pnpm-home`, `codex-install`, `opencode-install`, and `claude-install` directories directly under `<cache_dir>` are treated as one legacy generation. After publication, cleanup removes these directories only when no running or stopped container references the legacy runtime and no updated-AGS launch holds its legacy lease. User settings, authentication, `npm-global`, and other general caches are not removed. Upgrade the host AGS executable before updating: old launchers do not acquire leases, must not be started concurrently with cleanup, and cannot use legacy installations after they have been removed.

**Failed/interrupted-candidate cleanup:** when `update-agents` returns an ordinary error, it immediately discards that invocation's unselected candidate and uses a fresh container inspection to sweep older stale candidates. Cleanup warnings do not replace or hide the original update error. Repeated install or verification failures therefore do not accumulate candidates.

For crashes and forced termination, installation and verification containers hold a shared lock on their candidate's `.installing` marker, independently of the AGS parent process. A later invocation removes an incomplete candidate only when a fresh inspection finds no running/stopped container reference, its marker is at least 10 minutes old, and the marker can be locked exclusively. The grace period closes the crash window before Podman creates the container or acquires its lease. This also collects abandoned candidates created by earlier generation-aware AGS versions. Fresh, referenced, or leased candidates are retained.

Unrelated directories, symlinks, and malformed `.installing` markers are never cleanup candidates. Do not manually remove generation directories used by containers or pending launches.

The enabled set comes from `[sandbox].enabled_agents`; installer settings such as `pi_spec` come from `[update]`.
Use `--config <path>` when the selection was saved to a non-default config.

Security hardening and runtime hygiene:

- pnpm installs run with `ignore-scripts=true`. OpenCode does not use pnpm: AGS resolves its saved catalog source, downloads the architecture-specific release archive, verifies its SHA-256, and validates the staged binary version.
- `minimum_release_age` applies to pnpm package selection and catalog `latest` GitHub Release selection. Exact catalog versions never fall forward.
- Agent provider policies come from `agent_provider_lock`. When omitted, AGS uses its reviewed embedded five-agent defaults; `ags tools` writes a content-addressed lock for a custom selection.
- Interrupted installations never replace the selected generation; the next update builds afresh.
- Codex releases are stored in the generation's `codex-install` directory while its launcher remains at `/usr/local/pnpm/codex`.
- The updater-only pnpm store/cache are mounted at `/var/cache/ags/agent-pnpm-{store,cache}` only during installation. They persist without automatic `pnpm store prune`; verification runs with no network and mounts neither cache. Ordinary sandboxes cannot access these writable caches.
- `update-agents` removes stale pnpm self-update shims from `/usr/local/pnpm` so sandbox `pnpm` resolves to the image-provided pnpm binary.
- Legacy shared npm-global files are left untouched for existing sessions. Managed agent launchers use explicit paths, and managed runtime paths precede npm-global on the sandbox PATH.

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
- Managed shortcuts use `--defaults` where applicable so direct launches and generated wrappers stay in sync.
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
