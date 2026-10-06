# Architecture Overview

`ags` follows a simple pipeline:

1. Parse CLI args.
2. Load + validate config.
3. Prepare assets/secrets/ssh/git metadata.
4. Build a launch plan.
5. Render plan into `podman run` args.
6. Execute container.

---

## Main modules

- `cli.rs`
  - Defines command enums and parses args.
- `config/*`
  - TOML deserialization + validation into strongly typed config.
- `cmd/*`
  - Subcommand implementations (`setup`, `doctor`, `update`, etc).
  - The tool configurator validates DNF or verified-archive providers and materializes selected downloads as a lock beside the owning config.
- `agent.rs`
  - Agent-specific profiles (command, mounts, env, browser integration).
- `plan/*`
  - Converts config + runtime state into final `LaunchPlan`.
- `workspace_cache.rs` / `workspace_cache_{gc,usage,delete}.rs`
  - Records checkout-scoped pnpm cache identities and shared launch-plan leases.
  - Collection discovers orphan identities without Git subprocesses, refreshes running/stopped Podman mounts under a launch-registration gate, and quarantines only unreferenced/unleased trees after grace.
  - Deletes quarantined trees outside the gate with a filesystem-deletion budget, retaining coordination metadata for crash recovery. Normal launches do not run collection.
- `cmd/prune_workspace_caches.rs`
  - Low-priority, non-interactive maintenance entrypoint, intended for the daily systemd user timer or cron alternative in `config/`.
  - Reads only the selected host config and bypasses config bootstrap, repository overlays/trust prompts, secrets, and release-check networking.
- `podman/*`
  - Turns `LaunchPlan` into `podman run` arguments and executes.
  - Renders `podman build` arguments with explicit layer-cache and pull policies.
- `image_update/*`
  - The single image creation/update pipeline shared by `ags update-image` and first launch.
  - Resolves the recorded Fedora base and upstream metadata, reuses or builds content-keyed components (OS baseline and checkpoints, build foundation, Rust, pnpm, one artifact per vendor tool, Glimpse), assembles and verifies a candidate offline, and publishes it through a pending record under a per-image lock.
  - Keeps a small per-image state manifest in `~/.cache/ags/image-state/`; see `docs/COMMANDS.md` for the invalidation table.
- `ssh.rs`
  - Dedicated ssh-agent lifecycle + key loading.
- `secrets.rs`
  - Multi-source secret resolution.
- `auth_proxy/*`
  - Ephemeral auth proxy for sandbox browser opens and OAuth loopback callbacks.
  - `protocol.rs`: JSON-over-Unix-socket message types (`ShimMessage`, `HostMessage`).
  - `host.rs`: host-side proxy — Unix socket listener, user prompt via shared AGS dialog, callback relay.
- `host_dialog.rs`
  - Shared host prompt abstraction with Glimpse host-UI renderer first and zenity/kdialog fallback.
- `host_ui.rs`
  - Session-scoped host UI sidecar lifecycle for sandbox-safe Glimpse windows.
  - Starts `glimpse-host-ui`, waits for its Unix socket, and cleans it up on session end.
- `clipboard.rs`
  - Session-scoped clipboard bridge for sandbox `wl-paste`/`wl-copy` shims.
  - Reads/writes the host clipboard through a narrow Unix socket instead of exposing the compositor.
  - Gates clipboard contents reads through the shared host approval dialog by default.
- `agent/extensions/clipboard-paste/*`
  - Session-scoped Pi editor integration mounted read-only by `plan/clipboard.rs`.
  - Keeps one approval-gated read pending, then inserts image paths/text without replaying keystrokes or submitting a prompt.
  - Composes the current editor factory and discards cancelled or stale-session/draft results.
- `webview_relay.rs`
  - Session-scoped host HTTP relay for host-owned webviews that need to reach sandbox-local temporary app servers.
  - Pairs with embedded sandbox helper scripts written by `assets.rs`.
  - `glimpseui` is the intended owner of localhost-to-relay URL resolution for Glimpse-based packages.
- `assets.rs`
  - Writes embedded Containerfile and component recipes (reference copies; image builds use a private snapshot), tmux, system-wide uv policy, guard, settings, auth-proxy-shim, clipboard, and webview relay assets.
- `node_runtime.rs`
  - Validates numeric `.nvmrc` selectors and bounds nearest-file discovery to the workspace.
- `cmd/node.rs`
  - Implements the `ags node install|list` helper-container workflow and persistent mise store.
  - The store is read-only in ordinary agent containers; the install helper is its only AGS writer. Lockdown launch plans omit it and the runtime wrappers to retain ephemeral agent-runtime isolation.

---

## Execution model

### Run mode

- Immutable agent-generation mounts are read-only.
- Normal package-manager writes use a per-worktree pnpm store/cache; lockdown uses ephemeral storage. Launch plans retain shared cache leases until their last owner exits, protecting delayed launches from maintenance.
- The updater-only pnpm download cache is mounted only in installer containers and is absent from verification and runtime containers.
- User calls `ags --agent <name> ...`.
- Config is validated.
- Secrets are resolved and written to an env file.
- Mounts and env are assembled per agent profile.
- Container entrypoint script runs chosen agent command.

### Subcommands

- `setup`/`doctor`/`update`/`update-agents` operate as host-side utilities.
- `prune-workspace-caches` performs explicit orphan cleanup with grace, fresh container usage checks, leases, and bounded resumable deletion; it does not prune valid checkout caches or agent runtime generations. Scheduling is external and optional.
- `install` writes embedded assets and optional self-link; it does not install maintenance timers or cron jobs.
- `create-aliases` manages shell alias blocks and wrapper scripts.
- `completions` prints shell completion scripts (bash/zsh/fish).

---

## Key design constraints

- Rootless Podman execution.
- Principle of least privilege for mounts and env.
- Reproducible defaults via embedded assets.
- Config-driven behavior with validation before launch.
- Downloaded non-RPM tools use pinned architecture-specific HTTPS artifacts and are installed only after SHA-256 verification; DNF-provided tools are installed from configured Fedora packages.
- Agent state persisted on host volumes.
