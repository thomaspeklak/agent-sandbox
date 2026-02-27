# Agent Sandbox Migration Tasks

## Phase 0 — Foundation
- [x] Confirm language choice (Rust / Go fallback)
- [ ] Scaffold compiled launcher project
- [ ] Add CI build + lint + test pipeline
- [ ] Add migration docs + ADR for language decision

## Phase 1 — Config + Plan Engine
- [ ] Parse/validate existing config schema parity
- [ ] Implement normalized launch-plan model
- [ ] Implement mount expansion from `[[tool]]`
- [ ] Implement secret source resolution (`env`, `secret-tool`)

## Phase 2 — `pi` Runtime Parity
- [ ] Podman image ensure/build parity
- [ ] Gitconfig bootstrap parity
- [ ] Dedicated SSH agent parity
- [ ] Browser sidecar parity
- [ ] Wayland/clipboard parity
- [ ] External git metadata mount parity (worktree support)
- [ ] `ags --agent pi` command parity

## Phase 3 — Companion Commands Parity
- [ ] `ags setup` parity
- [ ] `ags doctor` parity
- [ ] `ags update` parity
- [ ] `ags install` / `ags uninstall` parity
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
