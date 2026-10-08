# T3 Integration Task Checklist

Requirements: [prompt.md](prompt.md).
Approved plan: [implementation_plan.md](implementation_plan.md).

Implementation and sandbox checks are complete on `feat/t3-integration`.
Live Podman/desktop checks remain pending because Podman is unavailable in the
development sandbox. OpenSSH lifecycle tests use a Podman protocol fixture;
the published pnpm T3 v0.0.45 bundle passed a separate permission-read-only
server/pairing/native-PTY smoke check. Keep implementation files within 500 lines.

## 1. Versioned SSH compatibility contract

- [x] Capture T3 v0.0.45 launch/reuse, pairing, forwarding, and teardown fixtures,
  including SSH commands and scripts transmitted through stdin.
- [x] Record exact runtime readiness, helper commands, discovery metadata,
  ownership, and requested-version behavior from the tagged release.
- [x] Define a bounded compatibility adapter for supported command/script forms
  that validates intent and requested version before execution.
- [x] Reject unknown bootstrap forms and desktop/runtime mismatches with clear
  errors before any installer or download runs.
- [x] Validate both the actual mounted runtime and the running server version.

## 2. Agent selection and pnpm updates

- [x] Split near-limit installer, updater, and launch modules before extending
  them; keep every touched Rust implementation file within the 500-line limit.
- [x] Add T3 to agent identifiers, installable-agent selection, CLI parsing,
  help, completions, configuration validation, and doctor.
- [x] Add the pnpm provider for npm package `t3` to the checked-in agent catalog
  and provider lock.
- [x] Integrate T3 with the Agent CLIs panel and selection/provider persistence
  while retaining explicit user selections and independent backing-provider
  selection.
- [x] Extend pnpm installation, removal, protected-package reconciliation,
  launcher verification, and disabled-agent handling using existing policies.
- [x] Extend inventory and mount selection for T3-only configurations.
- [x] Inventory the launcher, optional platform package, bundled native
  dependencies, web assets, resource monitor, and generated compatibility files.

## 3. Runtime and persistent storage separation

- [x] Generate the exact-version SSH runtime view from the pnpm-installed
  platform bundle during `update-agents`, including the executable and
  `.install-complete` marker.
- [x] Include the generated runtime view in verified-generation inventory and
  retention, preserving compatibility with existing runtime generations.
- [x] Bind the generation-owned runtime read-only at the repository T3 home's
  `.t3/runtime` path.
- [x] Persist writable `.t3/userdata`, `.t3/worktrees`, and `.t3/ssh-launch`
  separately from immutable runtime files.
- [ ] Verify runtime startup offline with read-only mounts and exercise native
  terminal functionality rather than relying only on `--version`.

## 4. Repository identity and registration

- [x] Resolve canonical common Git directory, main checkout, active checkout,
  and relevant worktrees into one repository environment identity.
- [x] Implement non-secret registration with stable container identity,
  ownership metadata, explicit configuration/overlay context, persistent T3
  storage, SSH alias/identity, and image/runtime metadata.
- [x] Validate ownership before container reuse and report conflicting
  configuration instead of creating duplicate repository environments.
- [x] Establish trust and configuration context during interactive registration.
- [x] Load the registered context explicitly for automatic startup and fail
  actionably when expected trust/configuration prerequisites are unavailable.
- [x] Keep registration, private SSH identity, lifecycle control, and Podman
  authority outside sandbox mounts.

## 5. Persistent AGS lifecycle owner

- [x] Add a long-lived host owner for the repository container, T3 server,
  runtime leases, applicable sidecar guards, and diagnostics.
- [x] Connect `ags --agent t3` to registration/preflight and owner start/attach.
- [x] Implement internal owner startup through the same AGS executable with
  independent stdio, readiness IPC, and startup/lifetime locking.
- [x] Serialize concurrent cold starts into one owner and one container.
- [x] Preserve the environment, T3 server, and active jobs after transport
  disconnect until explicit stop.
- [x] Implement explicit repository-scoped status, stop, and upgrade/recreate
  controls, including clean shutdown ordering.
- [x] Reconcile ownerless/stale environments before reuse and preserve mounted
  sidecar path consistency during recovery.
- [x] Reacquire credentials for fresh server boots without persisting resolved
  secrets or consuming SSH transport bytes for prompts.
- [x] Reuse or extend descriptor-based handoff for server startup where needed,
  preserving existing credential precedence and shared SSH-agent behavior.

## 6. Genuine host-side SSH bridge

- [x] Add russh transport over a private local stream and the managed SSH alias
  with `ProxyCommand` startup/reuse and byte relay.
- [x] Implement authentication and persistent host-key verification.
- [x] Implement command channels with stdin, separate stdout/stderr, EOF, exit
  status, cancellation, and readiness/failure propagation.
- [x] Dispatch validated desktop operations inside the selected container,
  without executing bootstrap scripts as host shell commands.
- [x] Implement `direct-tcpip` forwarding to the selected container's loopback
  service with correct backpressure and channel shutdown.
- [x] Start T3 under AGS ownership and expose compatible runtime/discovery state
  so the unchanged desktop reuses the server.
- [x] Preserve the AGS-owned server during desktop teardown and prevent late
  teardown from cold-starting an explicitly stopped environment.
- [x] Enforce disabled-T3 and version policies for bootstrap, pairing, and new
  connection requests before admitting work.

## 7. Worktrees and providers

- [x] Bind the repository-specific T3 home at its identical absolute host and
  container path.
- [x] Mount the main checkout, required external Git metadata, existing relevant
  worktrees, and the parent for future T3-created worktrees at identical paths.
- [x] Detect external worktree mount changes requiring recreation and report
  them without interrupting active jobs.
- [x] Configure T3-supported enabled providers using AGS-managed binaries and
  explicit executable paths/wrappers.
- [x] Restore existing `/home/dev` provider-home/configuration behavior without
  relocating shared authentication or history.
- [x] Preserve applicable AGS hooks, sandbox instructions, Git configuration,
  and provider permission selection.
- [x] Use the existing fixed-image Node bootstrap where the npm launcher needs
  it; keep general worktree-aware Node-version discovery deferred.

## 8. Generations, upgrades, and disable behavior

- [x] Pin the validated generation referenced by an existing environment under
  cleanup coordination, including recovery and explicit recreation transitions.
- [x] Preserve runtime retention for stopped containers.
- [x] Keep reconnect and ordinary stop/start on the environment's existing image
  and generation; do not switch active environments on `update-agents`.
- [x] Implement serialized explicit upgrade/recreation that adopts the selected
  image/runtime and preserves mounted T3 data, worktrees, and provider homes.
- [x] Omit disabled T3 from new generations and reject new startup/connection
  requests while retaining the agreed explicit-stop lifecycle for existing work.

## 9. Acceptance verification

- [x] Verify selection/provider-lock persistence, install/remove/re-enable,
  independent backing-provider selection, and T3-only runtime inventory.
- [ ] Verify offline read-only runtime startup, full bundle inventory, and native
  terminal readiness.
- [ ] Exercise an unchanged v0.0.45 desktop's cold bootstrap, server reuse,
  pairing, forwarding, and teardown against the bridge.
- [x] Prove exact mismatch and unknown bootstrap rejection cause zero installer
  or download execution, including pairing and an already-running server.
- [x] Verify concurrent cold starts and main/linked-worktree identity equality.
- [x] Verify configuration/trust prerequisites and credential acquisition do not
  consume or corrupt the SSH transport.
- [x] Verify disconnect survival, explicit stop, owner recovery, and late
  teardown after stop.
- [ ] Verify T3-created worktrees from both host Git/editor and sandbox paths,
  including Git references and missing/prunable status.
- [ ] Verify provider-home restoration, shared authentication/history, permission
  selection, and preserved applicable AGS integrations.
- [ ] Verify explicit recreation preserves mounted data and stopped environments
  retain the referenced runtime generation.
- [x] Verify disabling T3 rejects new work without deleting persistent state.

## 10. Documentation and completion

- [x] Update `README.md`, `docs/COMMANDS.md`, `docs/CONFIG.md`,
  `docs/TROUBLESHOOTING.md`, and relevant configuration examples.
- [x] Run `cargo fmt --check`.
- [x] Run `cargo clippy -p ags -- -D warnings`.
- [x] Run `cargo fetch --locked`.
- [x] Run `cargo test -p ags`, including the 500-line source-layout check.
- [x] Run `node --test agent/tests/clipboard-*.test.mjs` and relevant runtime
  inventory/compatibility tests.
- [ ] Complete live Podman/T3 smoke tests for the agreed lifecycle and data
  persistence behavior.
- [x] Record general Node worktree-version discovery in the deferred GitHub
  follow-up when that issue creation is authorized.
  https://github.com/thomaspeklak/agent-sandbox/issues/23
- [ ] Save the implementation walkthrough and update feature history after
  implementation and verification are complete.
- [x] Inspect `git status`, review the final diff, commit relevant implementation
  files, and push the branch.
  Implementation published as `8c44da6` on `origin/feat/t3-integration`.
