# Changelog

All notable changes to this project will be documented in this file.

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
