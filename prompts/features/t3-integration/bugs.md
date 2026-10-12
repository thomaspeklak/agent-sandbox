# PR #24 review fixes

Review: https://github.com/thomaspeklak/agent-sandbox/pull/24

- [x] Give missing T3 actions and `--repository` values accurate CLI errors.
- [x] Stop runtime preparation on package-path lookup failure or missing input.
- [x] Gate Linux-only sealed environment descriptors so macOS compiles.
- [x] Support T3 forwarding without the optional `socat` package.

## Regression coverage

- CLI parsing tests cover missing actions/paths and the separate
  `ags --agent t3` launch and `ags t3 status` management interfaces.
- Generated-shell and Node tests cover failed package lookup, missing/empty
  inputs, successful stdin execution, and side-effect-free imports.
- Descriptor tests cover JSON round-trip, rewind, and immutable seals on Linux,
  with a platform-gated Unsupported assertion for non-Linux hosts.
- Real TCP relay tests cover streaming, binary backpressure, both half-close
  directions, connection refusal, and cancellation. OpenSSH lifecycle coverage
  uses `extra_dnf_packages = []` and requires the bundled Node relay asset.

The relay's stdout uses a descriptor-backed socket stream to handle nonblocking
pipe backpressure on both Node 22 (CI) and Node 24 (the sandbox image). The large
binary-transfer regression reproduces the Node 22 failure of a file-stream writer.

## Live smoke fixes

- [x] Authenticate host GitHub release lookups with existing credentials and
  preserve HTTP rejection reasons, rate-limit reset times, and recovery actions.
- [x] Show scoped help for `ags t3 --help` and each management action.

## Desktop connection smoke fixes

- [x] Replace brittle full-script matching in desktop SSH bootstrap and pairing;
  cover equivalent request serialization and installer variants independently
  of AGS's template renderer.
- [x] Accept full Podman image IDs with or without the `sha256:` prefix during
  startup, status, reconnect, and upgrade, including existing saved metadata.
- [x] Explain the managed SSH alias and port handling in launch output and docs.
- [x] Reload the persistent owner on explicit upgrade so an updated AGS binary's
  SSH compatibility fixes take effect without manually killing the old owner.

Regression coverage exercises real OpenSSH launch/pairing with reduced desktop
request fixtures containing fatal installer code, harmless quote/whitespace/CRLF
changes, and exact-version mismatch checks. The installer never executes.
Lifecycle tests cover both Podman image ID formats, legacy saved metadata,
offline status, cold reconnect, malformed inspection, and owner replacement on
upgrade. Full Rust tests pass with `--test-threads=1` (parallel runs encountered
intermittent sandbox `ETXTBSY` in unrelated executable-fixture tests); formatting,
Clippy, and all 34 clipboard/T3 Node tests pass. Host desktop smoke verification
remains pending because this session runs inside the sandbox.

## Folder picker smoke fix

- [x] Start T3's folder picker in the mounted registered checkout instead of
  its isolated application home; verify prefix autocomplete through the native
  filesystem RPC, preserve user-selected picker settings, and explicitly set
  the server's working directory.

The regression failed before the fix because `addProjectBaseDirectory` was
missing. The lifecycle test now covers fresh setup, missing/empty legacy values,
explicit Podman exec workdir, and preserved custom settings through upgrade.
Running it with the published v0.0.45 platform binary also verifies the decoded
server configuration, directory browsing/prefix completion, regular-file
exclusion, and native PTY through real authenticated RPC. The full serialized
Rust suite, formatting, Clippy, 34 Node tests, and verifier shell syntax pass.
Desktop-UI/real-Podman mount verification remains a host-side smoke check.

## New PR #24 lifecycle feedback

- [x] Reacquire registered `--env` values before upgrade stops/replaces the owner;
  pass the captured values to the replacement and report missing names without
  interrupting the current environment.
- [x] Propagate owner-reported stop failures and timeouts; fall back to direct
  container stop only when the owner's control endpoint is absent/refused.
- [x] Refresh a browser-enabled container's persisted proxy command when a new
  sidecar receives a different port, retaining pinned image/runtime and mounted data.
- [x] Replace the cold-connection test's fixed owner-shutdown sleep with a bounded
  wait that verifies shutdown before concurrent SSH connections begin.

Six new lifecycle regressions and a bounded-response unit test cover these fixes.
The missing-values, owner-ERR, and stale-browser-port regressions were observed
failing before their respective fixes. Registered values are reacquired in memory
before stop/recovery and passed to the new owner; missing names leave the current
owner/container running. Stop and upgrade share strict absent/refused-endpoint
fallback and propagate operational errors/timeouts. Browser reconnect/recovery
refreshes the persisted command while keeping the image/runtime pinned and
mounted history/identity intact. Cold-start and recovery tests wait for the owned
process to exit rather than sleeping for an assumed shutdown duration.

Final validation passes: locked fetch, formatting, all-target Clippy, the complete
serialized Rust suite (292 unit tests and 24 T3 lifecycle tests), 34 Node tests,
and diff checks. Actual container/browser-proxy smoke verification remains a
host-side check; the regression uses a real loopback browser with the existing
Podman protocol fixture.
