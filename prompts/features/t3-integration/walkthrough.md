# T3 Integration Walkthrough

Implemented on `feat/t3-integration` from the approved plan. Sandbox validation
passes; live Podman mounts and the T3 desktop UI remain unverified because Podman
is unavailable in this development sandbox. The remaining live checks stay
unchecked in `tasks.md`, and feature history remains in progress.

## Agent installation and immutable runtime

- Added `t3` to agent parsing, installable selection, defaults, help, completions,
  provider validation, tool catalog, and doctor.
- Reused pnpm reconciliation with existing release-age, ignored-install-script,
  isolated-candidate, protected-package, and generation-publication policies.
- Extracted shared pnpm actions and runtime verification to small modules;
  split launch options and CLI error formatting before extending near-limit code.
- Added T3-only inventory and generated an independent complete platform bundle
  in `pnpm-home/ags-t3-runtime/versions/<version>`, with an exact-version marker.
- Added offline/read-only updater verification that starts T3, pairs, and opens,
  writes, resizes, and closes a native PTY through the actual v0.0.45 HTTP/RPC API.

## Repository identity, storage, and providers

- Resolve repository identity from canonical Git common metadata and discover
  main/linked worktrees through Git's machine-readable worktree inventory.
- Persist private non-secret registration, explicit config/trust context,
  absolute T3 home/control paths, and stable SSH identity outside sandbox mounts.
- Mount the checkout, required Git metadata, relevant existing worktrees, and
  the T3 home at identical host/container paths. Its already-mounted parent
  covers future default T3 worktrees.
- Bind the runtime namespace read-only while retaining writable T3 application
  data. Reject mount layouts exposing host SSH keys or lifecycle control.
- Configure enabled Codex, Claude, and OpenCode through managed wrappers with
  existing `/home/dev` homes. Preserve hooks, instructions, and T3 permission
  selection; do not inject unconditional permission bypass.

## Persistent owner and genuine SSH transport

- Added the long-lived owner and managed OpenSSH alias/ProxyCommand, using russh
  over a private Unix stream with public-key auth and stable host-key verification.
- Retain host sidecars, runtime leases, and jobs after transport disconnect.
- Added status, explicit stop, and explicit image/runtime recreation commands.
  Delayed desktop teardown acknowledges disconnect without starting/stopping
  the environment. Explicit stop leaves an idle control endpoint available.
- Serialize simultaneous cold startup and validate ownership/layout before reuse.
  Address command/forwarding operations through inspected container IDs.
- Reconcile stale owned containers and orphaned owner process groups before reuse;
  preserve stable sidecar bind paths across owner recovery.
- Recognize versioned upstream shell templates as data and dispatch typed
  operations. Incoming scripts, embedded installers, and downloaders never execute.
- Implement separate stdout/stderr, EOF, exit status, cancellation, and bounded
  loopback TCP forwarding. OpenSSH tests caught and fixed binary stdout buffering
  and child-pipe EOF handling.
- Resolve fresh credentials with bounded noninteractive lookups and sealed
  descriptors. Extend the existing 1Password handoff while keeping secrets out
  of durable registration, Podman container configuration, and argv.
- Add validated retained-generation pinning; updates leave active environments
  pinned, and explicit recreation preserves mounted data.

## Verification performed

- `cargo fmt --check`
- `cargo clippy -p ags -- -D warnings`
- `cargo fetch --locked`
- `cargo test -p ags`, including the 500-line implementation-file limit
- `node --test agent/tests/clipboard-*.test.mjs crates/ags/tests/t3_runtime.test.cjs`
- Real russh/OpenSSH authentication and framing tests, plus process-level
  lifecycle tests using a Podman protocol fixture: concurrent cold startup,
  reuse, pairing, TCP forwarding, mismatch with no installer calls, disconnect
  survival, stop, fresh credentials, upgrades, owner recovery, trust revocation,
  external worktree changes, and disabled-agent rejection.
- Real Git worktree identity/mapping and runtime-inventory mutation tests.
- Installed the published `t3@0.0.45` package through pnpm 12.6.0 in
  `/tmp/opencode`, with install scripts disabled and isolated caches. The full
  platform bundle passed server startup, pairing, and native PTY
  open/write/resize/close with permission-read-only runtime files. This is a
  standalone backend test, not a live Podman bind-mount or desktop-UI test.
- Regenerated the standalone Glimpse lockfile to match the workspace dependency
  resolution after adding SSH/runtime dependencies.

## Documentation and follow-up

Updated README, command/configuration/troubleshooting documentation and added
`docs/T3.md` covering registration, storage, versions, credentials, worktrees,
and lifecycle management.

General worktree-aware Node version discovery is tracked separately:
https://github.com/thomaspeklak/agent-sandbox/issues/23.

## Remaining live verification

Run the unchecked acceptance checks in `tasks.md` on a Podman-capable host with
the matching T3 desktop: updater `--network=none`/read-only bind verification,
desktop bootstrap/forwarding, host-and-container worktrees, provider auth/hook
behavior, active-job disconnect survival, and actual container recreation/data
retention. No implementation or verification claim marks those checks complete.
