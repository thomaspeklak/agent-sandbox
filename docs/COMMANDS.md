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
- `prune-workspace-caches`
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
15. Ensure the image exists (a missing image is created through the `ags update-image` pipeline; an existing image is used without update checks), then run `podman run`.
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
- If `[clipboard].enabled = true`, AGS starts a per-session clipboard bridge, mounts `/run/ags-clipboard`, shadows `wl-paste`/`wl-copy` with shims in `/home/dev/.local/bin`, and sets `XDG_SESSION_TYPE=wayland` for ordinary clipboard clients. For AGS-launched Pi, it also mounts a read-only, session-scoped paste extension and loads it with `-e`; it does not install it into your personal Pi home. Pi's configured clipboard-paste shortcut reads through the same approval-gated bridge, waiting up to five minutes for approval instead of abandoning the read after three seconds. Repeated paste presses while pending complete once; denial, Escape/Ctrl-C, editing/submitting the draft, or switching sessions cancels insertion. Images are inserted as private sandbox-local temporary file paths, like Pi's native paste, and text is inserted literally; the handler never submits a message. The host can still allow reads for `[clipboard].approval_seconds`. This integration is independent of guards/`--yolo`, and is inactive in lockdown and non-interactive Pi modes. `/copy`-style flows write through the shims when `mode = "readwrite"`.
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

Checks the sandbox image for updates and applies them incrementally, including the DNF and verified-download tools selected through `ags tools`.

```bash
ags update-image
ags update-image --rebase
ags update-image --keep-existing
ags update-image --config /path/to/config.toml
```

- **`ags update-image`** keeps the recorded Fedora base, checks RPMs, Rust stable, rustup, and pnpm for updates, applies changed vendor-tool locks, and reuses every unaffected component.
- **`--rebase`** refreshes the base image within the same Fedora release (never to another release), starts a new OS update lineage from the package baseline, refreshes the build foundation, and rebuilds the artifacts compiled against it. It restarts the lineage even if the registry returns the same digest, which collapses accumulated RPM update layers.
- `--keep-existing` keeps the superseded image for manual rollback/debugging.
- `--config` selects the base config file used for the build; a trusted repo-local overlay still takes precedence.
- Does **not** update agent CLIs installed in persistent volumes.
- Launching an agent never checks for updates. Only a missing image is created on launch, through the same path (the same base policy, verification, and publication) as `ags update-image`.

`ags update` remains as a deprecated alias for `ags update-image`.

### How updates stay small

The image is assembled from components that are cached and reused independently. Each component is identified by a content key over exactly the inputs that can change it, so an update rebuilds only what actually changed:

| Change | Rebuilt | Reused |
| --- | --- | --- |
| Nothing | nothing; the existing image is retained | everything |
| New RPM updates | one small OS checkpoint layer on top of the current one, then the final assembly | package baseline, Rust, pnpm, vendor tools, Glimpse |
| RPM metadata changed but no package changed | nothing | everything (the candidate checkpoint is discarded) |
| `extra_dnf_packages` changed | package baseline (new OS lineage), final assembly | Rust, pnpm, vendor tools, Glimpse |
| Same packages in another order or duplicated | nothing | the selection is sorted and de-duplicated |
| Rust stable release | Rust toolchain, Glimpse, final assembly | OS, pnpm, vendor tools |
| rustup release only | Rust toolchain (compiler kept), final assembly | Glimpse, OS, pnpm, vendor tools |
| pnpm release or a pin selecting a different release | pnpm, final assembly | OS, Rust, Glimpse, vendor tools |
| One vendor tool lock entry | that tool, final assembly | the other tools; unchanged archives are neither downloaded nor extracted again |
| Vendor tool removed | final assembly (its executable is absent from the new image) | the other tools |
| `uv.toml`, `tmux.conf`, or other late configuration (new AGS release) | final assembly only | every component |
| `--rebase` | build foundation, Glimpse, OS checkpoint (from the baseline), final assembly; the baseline too if the base digest changed | Rust, pnpm, and vendor tools, whose payloads do not depend on the base |

A normal run prints a compact summary, for example:

```text
Base:       retained recorded Fedora 44 digest
OS:         current; checkpoint reused
Rust:       current; artifact reused
rustup:     current
pnpm:       updated to 10.21.0; artifact rebuilt
Vendor:     7 reused, 1 changed
Glimpse:    reused
Image:      verified and published 3f2a9c1d7e40
```

### Base, packages, and tools

- The base is `registry.fedoraproject.org/fedora:44`. The first run records its digest and image ID, reusing a local copy and pulling only if none exists. Later runs use the recorded image, re-pulling it by digest if it was removed; if that fails, the error suggests `--rebase`.
- RPM updates are found with `dnf check-upgrade --refresh` (repositories may not be skipped as unavailable) and applied as a checkpoint on top of the previous checkpoint. A checkpoint is kept only if the installed-RPM inventory actually changed.
- Rust is the current stable release from the official channel manifest. It is installed by an archived, SHA-256-verified `rustup-init` and never self-updates. An unchanged compiler is kept when only rustup changes.
- pnpm defaults to the exact `latest` stable release from the npm registry. Set `[sandbox].pnpm_version = "12.9.1"` to pin an exact stable release, or `"latest"` to restore automatic updates. Ranges and prereleases are refused; unavailable pins fail without fallback. Both policies verify the registry's SHA-512 integrity and install without lifecycle scripts. First-time image creation uses the same policy; existing images change only through `ags update-image`.
- Vendor tools use the immutable architecture-specific URLs and SHA-256 values in `[sandbox].tool_download_lock`. Archives are downloaded on the host into a private verified store (`~/.cache/ags/image-build/downloads/`). Every stored entry is re-verified before reuse, and mismatched entries are deleted. Only the declared executable is extracted, from `zip`, `tar.gz`, or `tar.xz`. Two tools may not install the same command, and `pnpm` is reserved for the image itself.
- Malformed or unreachable metadata fails the update with the component's name. The existing image is kept, and AGS never reports it as current.

### Verification, publication, and recovery

- Components are built under private AGS names (`localhost/ags-build-<output-id>/<component>:candidate`); the configured image tag is never a build target.
- The assembled candidate is verified before publication with networking disabled and without host credentials, mounts, workspaces, or agent volumes. The check covers the `dev` user, paths and policy files, the selected RPMs and tools, mise, Node, pnpm, Rust/Cargo, rustfmt, clippy, Glimpse, and a tiny offline Rust build with and without the `sccache` wrapper.
- Any failure before publication leaves the configured image and its update state unchanged.
- Publication writes a pending record, moves the configured tag, confirms it, and then commits the state. If AGS is interrupted in between, the next run completes or discards the publication. If the image was changed outside AGS meanwhile, AGS leaves it untouched and reports it instead.
- Afterwards only the superseded final image is removed, with `podman image rm --no-prune` and without force. An image still used by a container, or still carrying another tag, is retained with a warning and the command to remove it later. Cleanup problems are warnings, never rollbacks. No global prune is performed.

### State, locking, and migration

- Update state is a small JSON manifest per output image, platform, and Podman storage, stored in `~/.cache/ags/image-state/` (honoring `XDG_CACHE_HOME`). It is independent of `[sandbox].cache_dir`, so configs that build the same image share one state.
- Updates of the same image are serialized with a lock in the same directory. A second run waits and then re-checks.
- Missing, corrupt, or unsupported state is a cache miss. Recorded components are re-inspected before reuse, never trusted blindly, and the working image stays in place until a verified replacement is published.
- The first run after upgrading from the monolithic image builds a new package baseline and every component once; the old image is not imported as an OS checkpoint.
- Builds read a private snapshot of the recipes embedded in the AGS binary. The `Containerfile` next to your config is a reference copy of the final-assembly recipe (the component recipes are written alongside it under `image/`). Editing it does not change what AGS builds.
- Superseded component images lose their AGS names and become dangling. `podman image prune` reclaims them when you choose.

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

Existing project `node_modules` trees may record the former `/usr/local/pnpm/.store` location and produce `ERR_PNPM_UNEXPECTED_STORE`. When the worktree is idle, reinstall its dependencies; AGS does not rewrite pnpm metadata or delete `node_modules` during launch. Run `ags update-image` to pick up the current stable pnpm release and matching image defaults.

Development caches and updater caches are separate trust domains: neither seeds the other, and no hard links are created across those boundaries. Published generations remain usable if either cache is deleted or unavailable. AGS rejects a writable workdir or configured mount whose host path is an ancestor of `agent-runtimes`, `agent-downloads`, or `workspace-caches`; otherwise the same protected files could be reached through a writable alias despite their dedicated mount policy.

### Running-session safety

Generations live under `<cache_dir>/agent-runtimes/generation-*`. The `current` file is an atomically replaced selection, not a container mount. Each new sandbox resolves it once and mounts concrete generation directories read-only. Existing sandboxes, including new agent processes started inside them, keep their original runtime.

Before installation, `update-agents` runs `podman ps --all --quiet --no-trunc` and `podman container inspect` for each container. It reports runtime generations referenced by actual mount sources and container status, including stopped containers that can be restarted. Legacy runtime mounts are reported too. Inspection errors abort the update rather than treating unknown usage as unused.

An OS file lock serializes updates for the same cache. Installation happens in a fresh generation; a second container checks each enabled CLI with a bounded `--version` smoke test against read-only runtime mounts. Only successful verification and a changed runtime manifest publish the selection. Failed/interrupted builds remain unselected, and do not advance the previous-generation pointer. Authentication and session data are not copied into generations.

**Automatic cleanup:** after a successful update or no-op check, AGS retains the latest generation, the previous successfully published generation, and every generation referenced by a running or stopped container. Other completed generations are deleted. Container references are inspected again immediately before cleanup; the pre-install snapshot is not reused. Inspection failures skip cleanup and emit a warning without undoing the successful update.

Pending launches hold a shared lease on their selected generation, so even a launch delayed across several updates is protected. Cleanup takes a short selection lock and only deletes generations whose lease it can lock exclusively. These leases do not block publication or cleanup of other generations. A launch-plan lease normally lasts until its Podman command exits; container inspection protects detached or stopped containers afterward.

**Legacy cleanup:** the old `pnpm-home`, `codex-install`, `opencode-install`, and `claude-install` directories directly under `<cache_dir>` are treated as one legacy generation. After publication, cleanup removes these directories only when no running or stopped container references the legacy runtime and no updated-AGS launch holds its legacy lease. User settings, authentication, `npm-global`, and other general caches are not removed. Upgrade the host AGS executable before updating: old launchers do not acquire leases, must not be started concurrently with cleanup, and cannot use legacy installations after they have been removed.

**Failed/interrupted-candidate cleanup:** when `update-agents` returns an error, it leaves that invocation's unselected candidate in place because a failed Podman client does not prove that its container has exited. A later invocation may safely collect the candidate after a fresh container inspection and lease check. Cleanup warnings do not replace or hide the original update error, so repeated failures can temporarily retain multiple candidates.

Installation and verification containers hold a shared lock on their candidate's `.installing` marker, independently of the AGS parent process. A later invocation removes an incomplete candidate only when a fresh inspection finds no running/stopped container reference, its marker is at least 10 minutes old, and the marker can be locked exclusively. The grace period closes the crash window before Podman creates the container or acquires its lease. This also collects abandoned candidates created by earlier generation-aware AGS versions. Fresh, referenced, or leased candidates are retained.

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

## `ags prune-workspace-caches`

Non-interactive, low-impact maintenance of orphaned development pnpm caches. It does **not** prune agent runtimes, updater downloads, project `node_modules`, user settings, or caches belonging to still-valid checkouts.

```bash
ags prune-workspace-caches --dry-run
ags prune-workspace-caches --config ~/.config/ags/config.toml
# Explicitly bypass the observed-orphan grace period after reviewing candidates:
ags prune-workspace-caches --dry-run --grace-days 0
```

An orphan is a checkout that disappeared or whose canonical path, Git metadata device/inode, or checkout incarnation changed. Non-Git directories and linked worktrees are supported. Inaccessible, malformed, or ambiguous identities are retained. Collection reads shallow identities only: no Git subprocesses, package-size scans, package-manager prune, or network requests.

CLI defaults (the systemd service overrides grace to one day):

- **Seven-day observed-orphan grace:** the first real maintenance run marks an orphan; collection starts only after seven days of subsequent observation. Launching the checkout again clears its marker. `--grace-days <n>` changes this, including explicit `0`. Dry runs neither start the grace clock nor delete caches.
- **Two cache trees and 1000 filesystem deletions per run**, adjustable with `--max-caches` and `--max-deletions` (minimum 3). Eligible trees are atomically quarantined, then deleted incrementally outside the launch gate. Large trees and interrupted runs resume next time. Symlinks are unlinked, never followed, and deletion refuses to cross filesystems. Budgets bound unlink work, not total directory entries or wall-clock time; use the systemd service or cron timeout below for a wall-clock cap.
- **CPU nice 19 and Linux idle I/O priority.** If these policies cannot be applied, maintenance fails before mutation. Idle I/O support depends on the kernel/filesystem/scheduler; deletion budgets remain in force regardless.
- **One fresh Podman inventory**, batched for running **and stopped** containers, only when there are eligible candidates. Any overlapping host mount protects the cache. Inspection has a five-second total deadline and fails closed. Shared launch-plan leases protect pending launches. Concurrent maintenance or a busy launch-registration gate skips successfully rather than waiting.

Maintenance reads only the selected **host config**, never repository overlays, and does not create a missing config, prompt for trust, resolve secrets, or check for AGS releases. Use `--config` for each distinct cache root. `--quiet` suppresses normal output but leaves errors visible.

### First-run output

```text
26 orphaned; 0 removed; 0 pending; 0 filesystem deletions
```

On a first real run, this normally means 26 caches were marked orphaned and are waiting for the observed-orphan grace period (one day in the systemd service; seven days for the CLI without `--grace-days`). It does not mean cleanup failed. Once grace expires, protected container/launch references can still prevent deletion. `pending` counts quarantined trees whose deletion budget ran out, not caches waiting for grace.

To explicitly bypass grace, review first, then run without `--dry-run`:

```bash
ags prune-workspace-caches --dry-run --grace-days 0
ags prune-workspace-caches --grace-days 0
```

All usage/lease checks and deletion budgets still apply. More than two eligible trees or large caches need subsequent runs; increasing budgets trades lower disk usage sooner for more per-run work.

### Scheduling with systemd (recommended)

Upgrade the host AGS executable before enabling collection. Older launchers do not acquire workspace-cache leases: do not launch them concurrently with cleanup. Existing running/stopped containers remain protected by inventory.

Install the **user** service and timer from a repository checkout, as the same user who runs AGS/Podman:

```bash
mkdir -p ~/.config/systemd/user
cp config/systemd/ags-prune-workspace-caches.service config/systemd/ags-prune-workspace-caches.timer ~/.config/systemd/user/
# Check `command -v ags`; edit the copied service's ExecStart if needed.
# Defaults: %h/.local/bin/ags and %h/.config/ags/config.toml.
systemd-analyze --user verify ~/.config/systemd/user/ags-prune-workspace-caches.service ~/.config/systemd/user/ags-prune-workspace-caches.timer
systemctl --user daemon-reload
systemctl --user enable --now ags-prune-workspace-caches.timer
```

The timer uses `OnCalendar=*-*-* 03:17:00`, `RandomizedDelaySec=15m`, and `Persistent=true`: one run daily around **03:17–03:32 local time**, with one catch-up run when the user manager returns after a missed schedule. The service explicitly sets **one-day observed-orphan grace** (`--grace-days 1`) and keeps the default two-tree/1000-deletion budgets, applies nice 19 and idle I/O priority plus low CPU/I/O weights, and limits runtime to 60 seconds with a five-second stop timeout. CPU/I/O weight enforcement depends on cgroup delegation. Control-group termination ensures systemd also terminates lingering subprocesses on timeout. Output goes to the user journal, not a shell profile or interactive prompt.

The one-day grace allows a temporarily missing checkout or mount to return while avoiding a week-long delay for rebuildable caches. Container and launch protections still apply. To update an existing installation, recopy the service and run `systemctl --user daemon-reload`; the enabled timer does not need restarting for this service-only change.

Check the next run and last result, or run the same protected cleanup now:

```bash
systemctl --user list-timers ags-prune-workspace-caches.timer
systemctl --user status ags-prune-workspace-caches.timer
journalctl --user -u ags-prune-workspace-caches.service -n 20
systemctl --user start ags-prune-workspace-caches.service
```

To stop future scheduled runs:

```bash
systemctl --user disable --now ags-prune-workspace-caches.timer
```

For execution while logged out, the user manager must keep running. Check `loginctl show-user "$USER" -p Linger`; if your machine's policy permits it, `loginctl enable-linger "$USER"` enables this. Without lingering, missed runs are caught up when the user manager starts again. No systemd timer is installed automatically by `ags install`.

The job must use the **same Podman connection/storage context** as your AGS launches. Preserve any custom `CONTAINER_HOST`, `CONTAINER_CONNECTION`, or container configuration environment in the user service; a different context cannot see the right containers. Use `systemctl --user edit ags-prune-workspace-caches.service` for local overrides and reload/restart the timer after scheduling changes. Keep each distinct cache root's `--config` explicit. Do not install these as root/system services.

### Scheduling with cron (alternative)

Use cron **instead of**, not in addition to, the systemd timer. `config/ags-workspace-cache-prune.cron.example` contains this daily, timeout-bounded user-crontab job:

```cron
PATH=/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin
17 3 * * * XDG_RUNTIME_DIR=/run/user/$(id -u) /usr/bin/timeout --kill-after=5s 60s "$HOME/.local/bin/ags" prune-workspace-caches --quiet
```

Adjust the executable/config paths and runtime directory to your environment, and preserve the same Podman context as your launches. If the user runtime directory or Podman context is unavailable, inspection fails and no cache is deleted. Unlike the persistent systemd timer, cron does not catch up missed runs. The first scheduled run marks orphans rather than immediately removing them. No cron job is installed automatically.

---

## `ags install`

Installs baseline assets and optional `ags` self-link.

```bash
ags install
ags install --link-self
ags install --link-self --force
```

### What it writes

- `~/.config/ags/Containerfile` (reference copy of the final-assembly recipe; component recipes go in `~/.config/ags/image/`)
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
