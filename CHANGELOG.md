# Changelog

All notable changes to this project will be documented in this file.

## [v0.8.0] — 2026-03-26

### Features

- feat(cli): add --root run flag for root-capable agent sessions (fe02d15)
- feat(config-editor): add TUI config editor (`ags config`) (fffa18d)
- feat(ags): add --stop-when-done flag for tmux mode (49b92fb)
- feat(auth-proxy): use host UI for proxied localhost URLs (fab1e12)
- feat(auth-proxy): add localhost proxy choice for browser opens (f64aaf8)
- feat(ags): add host UI and webview relay plumbing (1a6b635)

### Bug Fixes

- fix(root): add --user=root to podman args so root mode actually works (6c73314)
- fix(agent): remove apt from root mode prompt hint (Fedora-based image) (71b5390)
- fix(cli): merge duplicate run-flags sections in help output (4261ba6)
- fix(agent): symlink claude binary to prevent native-install startup warning (5f6b0bc)
- fix(auth-proxy): handle proxy dialog selection (529cd83)

### Chores / Other

- style: fix clippy warnings in config editor (38239ae)
- style: apply cargo fmt formatting (74653b0)
- refactor(ags): simplify and deduplicate across crate (-720 lines) (f22da62)

## [v0.7.0] — 2026-03-15

### Features

- feat(ags): support repo-local config overlays (c386ba9)
- feat: add guard yolo mode and dcg visibility (e96cb77)
- feat: integrate dcg into sandbox guards (10f1c7b)

### Bug Fixes

- fix: use EXIT trap for dcg temp file cleanup in guard hook (66b05f9)

### Chores / Other

- style: run cargo fmt (5151603)
- chore(beads): close agent-sandbox-l39 (49aa0f8)
- chore(git): stop tracking beads history exports (3597233)
- refactor: remove pi bash path heuristics (a6926f4)

## [v0.6.0] — 2026-03-13

### Features

- feat(psp): integrate podman-socket-proxy mode (bc2df72)

### Bug Fixes

- fix(pi): allow extensions by default (382ea01)

### Chores / Other

- chore: fix formatting and clippy issues (b1b3f7c)

## [v0.5.1] — 2026-03-12

### Bug Fixes

- fix(run): add runtime --add-dir flag (2f4f8f4)

## [v0.5.0] — 2026-03-12

### Features

- feat(install): add -m dir mount flag (05b2364)

### Bug Fixes

- fix(auth-proxy): support sockaddr_in on macos (99990cc)

### Chores / Other

- chore(beads): update issue state (366febb)
- chore(git): ignore beads history exports (11a8a06)

## [v0.4.0] — 2026-03-12

### Features

- feat: add update-available notification via GitHub releases (4b2609a)
- feat: add ephemeral auth proxy for sandbox browser opens and OAuth callbacks (616a02f)
- feat(guard): add Claude Code PreToolUse guard hook and plugin (5afd84d)

### Bug Fixes

- fix: resolve fmt clippy and test issues (32393e9)
- fix: set HOME/PATH explicitly for Claude install fallback (#3) (30d535d)

### Chores / Other

- chore: add beads issue tracker (00e3844)
- merge: integrate feat/claude-guard-hooks into main (6d7b545)
- merge: resolve conflicts with main (auth proxy + guard hooks) (8cf2b4f)
- Dim sandbox-on indicator and add JDK (aa1d26f)

## [v0.3.0] — 2026-03-10

### Features

- feat: add tmux sandbox support (0891d30)
- feat(run): inject concise host-service hint into agent prompts (0d6a420)
- feat(sandbox): add psql client and Postgres quick-connect docs (aece58c)
- feat(run): inject host-service runtime hints in sandbox (0d73e41)
- feat(update): bundle br/bv releases into sandbox image (4b6f1f0)
- feat(guard): move sandbox indicator out of footer (ab34af6)

### Chores / Other

- docs: clarify host service access from sandbox (db0f9d1)

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
