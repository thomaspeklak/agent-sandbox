# Walkthrough

`ags update-agents` builds a shell script in
`crates/ags/src/cmd/update_agents.rs`. The script writes `ignore-scripts=true`
to pnpm's configuration and installs `opencode-ai` globally. OpenCode ships a
failure stub as its initial executable and relies on `postinstall.mjs` to copy
the correct platform binary over that stub. The existing `command -v` check
only confirms that the stub exists, so the update succeeds even though the
runtime command fails.

AGS now resolves the global package directory with `pnpm root -g`, explicitly
runs `opencode-ai/postinstall.mjs`, and executes `opencode --version` before
continuing. Other package lifecycle scripts remain disabled. The
`opencode_postinstall_runs_before_runtime_validation` regression test verifies
this ordering in the generated update script.
