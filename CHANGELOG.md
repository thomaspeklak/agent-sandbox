# Changelog

All notable changes to this project will be documented in this file.

## [v0.2.0] — 2026-03-06

### Features

- feat(guard): surface sandbox mode and add AGS_SANDBOX marker (35fecae)
- feat(install): add --add-agent-mounts bootstrap option (338f8c3)
- add shell completion generation for bash zsh and fish (d6d2ebf)

### Bug Fixes

- ags: stop forcing PI_CODING_AGENT_DIR for pi (48f2be6)

### Chores / Other

- refactor(config): replace implicit agent sandboxes with explicit agent_mounts (c81bd0e)
- chore(security): disable npm/pnpm lifecycle scripts in sandbox (2f63cd8)
- docs: describe explicit agent_mount-based state and setup (2b98eab)

## [v0.1.2] — 2026-03-05

### Bug Fixes

- Fixed a Claude regression where the generated `/usr/local/pnpm/claude` wrapper forced `HOME=/opt/claude-home`, causing Claude to ignore mounted `/home/dev/.claude` state and show first-run onboarding.
- Updated the generated Claude wrapper to preserve runtime `HOME` and only prepend `/opt/claude-home/.local/bin` to `PATH`.
- Added regression tests for `ags update-agents` script generation to ensure update/install still use persistent Claude install paths while runtime `HOME` remains untouched.

## [v0.1.1] — 2026-03-05

### Bug Fixes

- Made `ags update-agents` robust for Claude updates by forcing persistent Claude home/path (`/opt/claude-home`) during update/install.
- Added fallback reinstall via `install.sh` when `claude update` fails.
- Replaced Claude shim in `/usr/local/pnpm/claude` with a wrapper that always exports persistent `HOME` and `PATH`, so `claude` in `--agent shell` uses the updated persistent installation.

## [v0.1.0] — 2026-03-05

### Features

- Rust rewrite of the sandbox launcher CLI (`ags`) with rootless Podman execution.
- Multi-agent runtime support: `pi`, `claude`, `codex`, `gemini`, `opencode`, and `shell`.
- Config-driven mounts, tool wiring, secret resolution, SSH bootstrap, and browser sidecar support.
- New release automation via GitHub Actions on `v*` tags.

### Bug Fixes

- Added external git metadata mount handling for linked worktrees/submodules.
- Improved install/update flows and sandbox bootstrap behavior.

### Chores / Other

- Project rename from `pi-sandbox` to `agent-sandbox`.
- Expanded user and contributor documentation (`README`, `docs/*`, `CONTRIBUTING.md`).
- Added reusable release prompt under `.pi/prompts/release.md`.
