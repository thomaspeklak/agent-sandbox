# Agent Sandbox Migration (Overview)

Status: 🟢 M1/M2 Implementation Complete
Last updated: 2026-02-27

## Goal

Migrate from **pi-only sandbox** to a generalized **agent sandbox** with:

```bash
ags --agent <pi|claude|codex|gemini|opencode> -- [args...]
```

while preserving full behavior parity for today’s `pis` workflow first.

---

## Coarse Progress

- Overall migration: **75%** (M1+M2 code complete, acceptance testing needed)
- Parity milestone (M1: `ags --agent pi`): **100%** code, needs acceptance testing
- Companion commands (M2): **100%** code, needs acceptance testing
- Multi-agent milestone (M3): **0%**

### Milestones

- [x] Planning baseline documented
- [x] M1: `ags --agent pi` launcher parity (code complete, 127 tests)
- [x] M2: `setup/doctor/update/install` parity (code complete)
- [ ] M3: add `claude`, `codex`, `gemini`, `opencode`
- [ ] M4: rollout + deprecation of shell-heavy path

---

## Current Focus (Now)

- [x] Confirm implementation language (**Rust**; Go fallback only if blocked)
- [ ] Define config schema v2 (scalable agent overlays)
- [x] Scaffold `ags` CLI (`--agent ... -- ...`)
- [ ] Acceptance testing: run `ags --agent pi` against real config
- [ ] Start parity test harness (old shell vs new launcher)

---

## Detailed Tracking

- Execution plan and phased tasks: [tasks.md](./tasks.md)
- Feature parity checklist: [parity-checklist.md](./parity-checklist.md)
- Open decisions: [open-questions.md](./open-questions.md)
- Rust implementation rules: [rust-guidelines.md](./rust-guidelines.md)
- Archived full draft plan: [archive/2026-02-27-draft-plan.md](./archive/2026-02-27-draft-plan.md)

## Loop Prompts (poor-man's Ralph loop)

- Implementation prompt: `./implementation-prompt`
- Review/fix prompt: `./review-prompt`

Example (fish):

```fish
for i in (seq 1 5)
  cat implementation-prompt | pis
  cat review-prompt | pis
end
```

Example (bash/zsh):

```bash
for i in {1..5}; do
  cat implementation-prompt | pis
  cat review-prompt | pis
done
```
