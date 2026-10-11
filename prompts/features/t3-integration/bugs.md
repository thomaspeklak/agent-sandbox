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
