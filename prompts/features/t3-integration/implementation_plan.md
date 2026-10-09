## Reconciled plan for approval

The architecture remains:

**T3 desktop → managed SSH alias/ProxyCommand → host AGS owner and genuine SSH bridge → repository container → T3 server and providers.**

Each repository has one environment and one saved T3 connection. Its worktrees share that environment; multiple repository environments appear in the same desktop UI.

### 1. Establish the versioned SSH compatibility contract

Use **T3 v0.0.45** as the initial compatibility baseline and capture fixtures for launch/reuse, pairing, forwarding, and teardown.

The adapter must account for these verified behaviors:

- Bootstrap explicitly uses `$HOME/.t3`.
- It prepares the requested exact-version runtime **before** checking for an existing server.
- Readiness requires an executable at `runtime/versions/<version>/t3` and a `.install-complete` file containing that version.
- Bootstrap scripts arrive through SSH stdin, alongside commands such as `sh -l -s`.
- Pairing independently invokes the runtime.
- Desktop teardown can issue a remote stop command.

Validate supported command/script forms and the requested version before execution. Unknown bootstrap forms and version mismatches must produce actionable errors before any installer or download runs.

Compare against the environment’s **actual mounted runtime and running server version**, rather than merely the newest global generation.

### 2. Integrate T3 into agent selection and updates

Add `T3` throughout the existing agent machinery:

- `crates/ags/src/cli_agent.rs`, help, completions, configuration validation, and doctor.
- The **Agent CLIs** panel in `ags tools`.
- `config/tool-packages.example.json` and `config/default-agent-providers.lock.json`.
- pnpm installation, removal, protected-package reconciliation, and runtime verification.

The provider uses npm package **`t3`**, installed through pnpm with Pi’s existing policies: release age, disabled lifecycle scripts, isolated candidates, and verified-generation publication.

Preserve explicit user selections. Selecting T3 does not automatically enable backing providers.

Extend inventory and mount selection to cover **T3-only configurations**. Current checks in `update_agents_identity.rs`, `update_agents_manifest.js`, and `plan/build.rs` hardcode existing pnpm agents.

Inventory the complete published runtime: launcher, optional platform package, bundled native dependencies, web assets, resource monitor, and generated SSH compatibility files.

### 3. Separate immutable runtime from persistent T3 data

Give T3 a repository-specific home at a real host absolute path, bind-mounted at that **same path** inside the container.

Under that home:

| Location | Treatment |
|---|---|
| `.t3/runtime` | Read-only view of the pnpm-installed bundle, owned by the selected runtime generation |
| `.t3/userdata` | Persistent writable application state |
| `.t3/worktrees` | Persistent writable parent for future T3-created worktrees |
| `.t3/ssh-launch` | Writable compatibility state |

Generate the versioned runtime view during `update-agents`, including the executable and completion marker. Verify its contents offline and read-only before publication.

Read-only mounts enforce runtime ownership, while the compatibility adapter provides immediate mismatch errors. Mount failures alone are insufficient because upstream installation-lock retries can delay failure.

### 4. Register one environment identity per repository

Introduce repository registration based on Git’s canonical **common directory**, with main-checkout and linked-worktree discovery. Launching from either resolves to the same registration.

Store non-secret metadata:

- Repository identity and checkout/worktree paths.
- Stable container name and ownership labels.
- Explicit global configuration and registered repository-overlay context.
- Persistent T3 storage.
- SSH alias and host identity.
- Selected environment image/runtime metadata.

Validate ownership before container reuse. Conflicting launch configuration for the same repository must be reported rather than creating another environment.

Interactive registration establishes the configuration context and existing trust prerequisites. Automatic startup loads that context explicitly; it must not depend on the SSH process’s working directory or silently omit an expected overlay.

### 5. Add a persistent AGS lifecycle owner

The current `lifecycle.rs` keeps sidecar guards within one invocation, and `podman/args.rs` renders disposable attached containers. Persistent T3 therefore needs a dedicated lifecycle.

A long-lived host AGS owner supervises:

- The repository container and T3 server.
- Runtime leases.
- Applicable browser, authentication, clipboard, host UI, and other sidecars.
- Startup locking, control IPC, readiness, and diagnostics.

`ags --agent t3` performs registration/preflight and starts or attaches to that owner. SSH-triggered startup launches the same AGS executable in an internal owner mode when needed.

Required behavior:

- Concurrent cold connections produce one owner/container.
- Disconnect preserves the server and active jobs.
- Explicit stop shuts down the environment.
- Ownerless/stale environments are reconciled before reuse.
- Fresh server boots reacquire credentials.
- SSH transport bytes are never consumed by credential or trust prompts.

Keep durable registration free of resolved secrets. Reuse the existing descriptor-based handoff machinery where appropriate for server startup.

Add repository-scoped **status, stop, and explicit upgrade/recreate controls**.

### 6. Implement the genuine host-side SSH bridge

Use **russh** over a private local stream, with a managed SSH alias whose `ProxyCommand` starts/reuses the owner and relays protocol bytes.

Support the capabilities T3 requires:

- Authentication and stable host-key verification.
- Command channels, stdin, separate stdout/stderr, EOF, and exit status.
- Cancellation and readiness errors.
- `direct-tcpip` forwarding to the selected container’s loopback service.

Execute validated remote operations inside the container. Bootstrap scripts must never run as host shell commands.

Start T3 under AGS ownership and expose the expected runtime/discovery state so desktop bootstrap reuses it. Desktop teardown must preserve that server. A late teardown request after explicit AGS stop must not cold-start the environment again.

### 7. Complete worktree and provider integration

Mount at identical absolute host/container paths:

- The main checkout.
- Required external Git metadata.
- Existing relevant worktrees.
- The persistent parent for future T3-created worktrees.

Verify host/container Git references in both directions. Newly created worktrees must remain usable by host Git and editors and must not appear missing or prunable.

Detect external worktree mount changes that require container recreation and report them clearly without interrupting active jobs.

For T3-supported providers:

- Use enabled AGS-managed binaries through explicit executable paths/wrappers.
- Restore existing `/home/dev` provider-home/configuration behavior.
- Preserve applicable AGS hooks, sandbox instructions, Git configuration, and shared authentication/history.
- Respect T3’s permission selection instead of blindly reusing permission-bypass launch flags.

Use the existing fixed-image Node bootstrap where the npm launcher needs it. General worktree-aware Node-version discovery remains deferred to a GitHub follow-up.

### 8. Preserve generation and upgrade semantics

`update-agents` publishes verified generations without switching active environments.

Reconnect and ordinary stop/start retain the existing environment’s image and mounted generation. Explicit upgrade/recreation adopts new ones while preserving mounted T3 data, worktrees, and existing provider homes. The unmounted writable layer may be replaced.

Extend runtime leasing to pin the validated generation already referenced by an existing environment. Retain protection for stopped containers.

Disabling T3 omits it from new generations and rejects new T3 startup/connection requests, while existing work follows the agreed explicit-stop lifecycle.

## Verification and implementation structure

Acceptance coverage will include:

- Selection, install/remove/re-enable, and T3-only inventory.
- Offline, read-only startup and native terminal functionality.
- Unmodified v0.0.45 desktop bootstrap, pairing, forwarding, and teardown.
- Exact mismatch and unknown bootstrap rejection with **zero installer/download execution**.
- Concurrent cold starts and main/linked-worktree identity equality.
- Disconnect survival, explicit stop, owner recovery, and late teardown.
- Host/container worktree interoperability.
- Provider-home restoration and retained AGS integrations.
- Recreation preserving mounted data.

Keep new Rust modules focused and **at or below 500 lines**, with separate test files. Split near-limit modules before extending them: `update_agents_script.rs` is currently 497 lines, `update_agents.rs` 475, and `plan/build.rs` 472.

Validation will follow `CONTRIBUTING.md`: formatting, Clippy, locked dependency fetch, AGS tests, relevant Node tests, and live Podman/T3 smoke tests. Update the README and command, configuration, and troubleshooting documentation.
