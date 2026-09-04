# Walkthrough

AGS now treats agent CLIs as a selectable runtime bundle separate from image
tools. `[sandbox].enabled_agents` accepts `pi`, `claude`, `codex`, `gemini`, and
`opencode` in any order, validates them into canonical order, and allows an empty
list for shell-only use. Omitting the field preserves the previous behavior by
enabling all five agents. Shell remains unconditionally available and is not a
valid list entry.

The `ags tools` TUI keeps its profession tabs and opens a separate Agent CLIs
panel with `a`. Saving writes image-tool and agent selections to the same config
layer, reports both sets of changes, and prints config-specific `update-image`
and `update-agents` commands. `ags update-agents --config <path>` uses that same
config and any trusted repository overlay.

`ags update-agents` installs or updates enabled runtimes and removes disabled
packages, launchers, and dedicated install files from persistent cache volumes.
Current-package removal is strict and verifies both pnpm launcher locations;
best-effort cleanup is limited to obsolete package names. Authentication and
settings under agent home mounts are not deleted, and the shared pnpm store is
pruned after reconciliation.

Launch preflight rejects a disabled agent after config loading but before assets,
secrets, sidecars, or Podman work. Plan construction excludes disabled agents'
runtime caches and known `[[agent_mount]]` homes while retaining resources for
every enabled agent, preserving cross-agent workflows. Setup, doctor, and shell
launch asset preparation also skip disabled Pi and Claude integrations.

Regression coverage includes legacy/default, subset, and shell-only config
behavior; validation failures; TUI state and persistence; strict installer
cleanup; custom-config command parsing; launch rejection; mount isolation; and
conditional doctor/lifecycle behavior. `cargo fmt --all --check`,
`cargo clippy -p ags --all-targets -- -D warnings`, `git diff --check`, focused
tests, and all feature-related regression tests pass. The full AGS suite has one
unchanged environment-specific failure in
`secrets_resolve::os_runner_kills_and_reaps_timed_out_helper`, where a killed
descendant remains visible in this container. Networked agent installation is
verified through generated command tests rather than executed in normal CI.
