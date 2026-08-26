# Selectable Agent CLIs

Extend the `ags tools` configurator so users can choose which supported agent
CLIs are installed and exposed in the sandbox runtime.

## Requirements

- Keep the existing three profession tabs for image tools unchanged.
- Add a separate Agent CLIs panel to the same TUI.
- Support Pi, Claude, Codex, Gemini, and OpenCode. Shell remains always available
  and is not selectable.
- Persist the selection in `[sandbox].enabled_agents`.
- Existing configurations that omit `enabled_agents` must continue to enable all
  current agents.
- Reject unknown, duplicate, and `shell` entries.
- `ags update-agents` must install or update selected agents and remove deselected
  agent runtimes while preserving their host authentication and settings.
- Launching a deselected agent must fail before Podman or other host-side launch
  work starts, with actionable remediation.
- Do not expose runtime caches or `[[agent_mount]]` entries belonging only to
  deselected agents. Preserve the current behavior for all enabled agents so
  existing cross-agent workflows continue to work.
- Keep CLI agent names, aliases, completions, cache paths, and `[[agent_mount]]`
  syntax backward compatible.
- Clarify in the UI and documentation that agent CLIs live in persistent runtime
  volumes rather than in the base container image.
