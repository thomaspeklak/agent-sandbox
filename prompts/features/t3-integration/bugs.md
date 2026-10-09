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
