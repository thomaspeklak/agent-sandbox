# Walkthrough

## Catalog And Providers

- Extended canonical tool definitions with mutually exclusive DNF and verified
  archive providers.
- Added strict validation for pinned versions, exact `x86_64` and `aarch64`
  artifacts, credential-free HTTPS URLs, SHA-256 digests, archive formats,
  executable members, installed command names, and duplicate ownership.
- Added Terraform 1.15.9 and OpenShift CLI 4.22.9 as verified vendor downloads.
- Added Ansible Playbook, kubectl, AWS CLI, Helm, `dig`, hcloud, uv, and Black
  through Fedora 44 package names.
- Kept every requested tool optional and kept curl in the fixed image baseline.
- Added the approved infrastructure, orchestration, cloud, network, package
  manager, and code-quality placements.

## Reproducible Locks

- Added validated `LockedToolDownload` records and the managed
  `[sandbox].tool_download_lock` field.
- Materialized selected downloads into SHA-256-addressed JSON lock files beside
  the config layer that owns tool selection.
- Stored lock references as relative paths and resolved them from the declaring
  base or repo-local config directory for portable overlays.
- Kept previous immutable locks intact until the config atomically references a
  new lock, preventing partial saves from changing active build input.
- Preserved unknown locked tools unless a catalog owns their ID or a selected
  catalog download owns the same installed command.
- Preserved effective base packages when an overlay owns only the download lock.

## Image Installation

- Propagated validated lock records through launch plans, automatic image builds,
  and explicit `ags update-image` builds as base64-encoded structured input.
- Added architecture selection, HTTPS-only downloads, timeouts, retries, SHA-256
  verification, exact member extraction, executable installation, and cleanup.
- Ensured `set -e` applies within every installer loop iteration instead of
  placing the loop in a conditional shell list.
- Rejected option-like and wildcard archive members in both Rust and Containerfile
  validation and terminated tar options with `--`.

## uv Policy

- Added an embedded `config/uv.toml` build-context asset copied to
  `/etc/uv/uv.toml` in the image.
- Set `exclude-newer = "1 week"` to delay newly uploaded registry artifacts.
- Explicitly retained uv's dependency-confusion-resistant
  `index-strategy = "first-index"` behavior.
- Enabled `[pip].verify-hashes = true` so provided requirement hashes are checked
  without requiring hashes for every workflow.
- Avoided global wheel-only, mandatory-hash, source-override disabling, and preview
  malware settings that would break common projects or add service availability
  dependencies.

## Documentation And Tests

- Updated README, quick setup, command, configuration, architecture, and Python
  tooling documentation.
- Added catalog, provider, lock parsing, overlay ownership, persistence,
  immutability, installer hardening, uv policy, build argument, and image asset
  regression coverage.
- Split existing update tests and TOML merge helpers into focused files to retain
  the enforced 500-line implementation limit.
- Recorded and resolved every focused security-review item in `bugs.md`.

## Verification

- `cargo test -p ags`
- `cargo clippy -p ags --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `npm run check --prefix website-tools`
- `uv --config-file config/uv.toml cache dir`
- `git diff --check`
- Final focused correctness and security review reported no remaining Critical or
  Warning findings.
- Container smoke testing remains unavailable because Podman is not installed on
  this host.
