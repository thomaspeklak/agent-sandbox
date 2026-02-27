# Agent Sandbox Migration Tasks

## Phase 0 — Foundation
- [x] Confirm language choice (Rust / Go fallback)
- [x] Scaffold compiled launcher project
- [ ] Add CI build + lint + test pipeline
- [ ] Add migration docs + ADR for language decision

## Phase 1 — Config + Plan Engine
- [x] Parse/validate existing config schema parity
- [x] Implement normalized launch-plan model
- [x] Implement mount expansion from `[[tool]]`
- [x] Implement secret source resolution (`env`, `secret-tool`)

## Phase 2 — `pi` Runtime Parity
- [x] Podman image ensure/build parity
- [x] Gitconfig bootstrap parity
- [x] Dedicated SSH agent parity
- [x] Browser sidecar parity
- [x] Wayland/clipboard parity
- [x] External git metadata mount parity (worktree support)
- [x] `ags --agent pi` command parity

## Phase 3 — Companion Commands Parity
- [x] `ags setup` parity
- [x] `ags doctor` parity
- [x] `ags update` parity
- [x] `ags install` / `ags uninstall` parity
- [ ] Keep `pis*` wrappers for compatibility

## Phase 4 — Additional Agent Adapters
- [ ] `claude` adapter
- [ ] `codex` adapter
- [ ] `gemini` adapter
- [ ] `opencode` adapter
- [ ] Publish adapter capability matrix

## Phase 5 — Hardening + Rollout
- [ ] Security review (mount/env boundaries)
- [ ] Backward compatibility tests with existing configs
- [ ] Dogfood period with `--agent pi`
- [ ] Controlled rollout for non-pi agents
- [ ] Legacy shell path deprecation plan
