# Implementation Plan

1. Add `[sandbox].enabled_agents` with valid values `pi`, `claude`, `codex`,
   `gemini`, and `opencode`. Treat omission as all five for backward compatibility,
   allow an empty list for shell-only use, reject invalid or duplicate entries, and
   serialize selections in canonical order.
2. Extend `ags tools` with a built-in Agent CLIs panel while preserving the existing
   three profession tabs. Show selected counts, support keyboard toggling and default
   restoration, and persist tool and agent choices as one configurator-owned bundle.
3. Update `ags update-agents` to reconcile the selected set: install or update only
   selected agents, remove deselected runtime packages and launchers, prune the shared
   pnpm store, preserve host authentication/settings, and report updates and removals.
4. Reject launches of disabled agents immediately after config loading and before
   assets, secrets, sidecars, or Podman work. Keep shell unconditionally available and
   retain all supported agent names in static help and completions.
5. Exclude runtime caches and `[[agent_mount]]` entries that belong only to disabled
   agents while preserving mounts for every enabled agent. Keep generic development
   caches and explicit `[[mount]]` entries unchanged.
6. Make Pi/Claude setup and doctor behavior conditional on enabled agents, expose the
   new advanced sandbox field in `ags config`, clarify that the existing Agents editor
   controls home mounts, and update embedded defaults, examples, help, and docs.
7. Add regression coverage for config compatibility and validation, configurator state
   and persistence, installer reconciliation, launch rejection, mount filtering, and
   conditional setup/doctor behavior.
8. Run formatting, Clippy with warnings denied, the full AGS test suite, diff checks,
   and final review. Record that networked agent installation is covered by generated
   command tests rather than executed in normal CI.
