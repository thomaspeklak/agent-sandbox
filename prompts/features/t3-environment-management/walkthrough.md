# Global T3 management walkthrough

## Commands and discovery

- Public management defaults to the host registry, independently of the current
  directory. An empty registry returns an empty status array without creating
  storage, configuration, or services.
- Global status is a JSON array; an environment ID/full repository identity or
  `--repository` selects one JSON object. The displayed stable `environment` ID
  equals the managed SSH alias. Ambiguous aliases require the full identity.
- Inventory retains existing ownership/identity checks, refuses symlinked entries,
  and reports invalid registrations separately. It observes owner, server, and
  owned-container state without starting or repairing anything. Owner status has
  a two-second read timeout; normal owner shutdown is treated as offline.
- Owner status checks whether the server process has exited instead of reporting
  only that an in-memory environment structure exists.

## Selection and actions

- Global stop/upgrade use the existing Crossterm/Ratatui dependencies for a
  scrolling unchecked checkbox list showing environment IDs, repositories,
  aliases, and runtime versions. Up/Down or j/k navigates; Space toggles;
  Enter applies. Escape, Ctrl-C, and an empty selection change nothing.
- Terminal state is restored before actions. Global interactive selection uses
  stdin/stderr; scripts must select an environment/path or explicitly pass
  `--all` to act on every working instance.
- Stopped/unavailable instances appear in status but are excluded from global
  actions. Explicit targeting retains stopped-instance upgrade and recovery.
- Recheck working state before each selected action, so a stopped instance is
  not unexpectedly restarted from an outdated checklist. Execute sequentially,
  continue after individual failures, report outcomes, and return nonzero on
  any selected failure. Existing mounted-data and SSH-identity preservation stay
  within the existing lifecycle implementation.

## Documentation and regression coverage

- Updated scoped CLI help, Bash/Zsh/Fish completions, README, command reference,
  T3 lifecycle documentation, and troubleshooting guidance. The original T3
  integration walkthrough links this follow-up.
- Added selector parsing, conflict rejection, checkbox model/rendering, and
  executable Bash-completion coverage.
- Extended the Podman protocol fixture to represent independent containers for
  multiple registered repositories. Existing lifecycle tests now explicitly
  select their repository; fixture teardown cleans up all recorded owners.
- Added ten management integration tests covering global read-only discovery,
  single-target isolation, stopped upgrade, bulk filtering, malformed/symlinked
  registrations, ambiguous IDs, non-UTF-8 filenames, bounded owner status,
  partial-failure continuation, data/key retention, real pseudo-terminal
  confirmation/cancellation, and a stop while the checklist is open.

## Verification

Passed `cargo fetch --locked`, `cargo fmt --check`,
`cargo clippy -p ags --all-targets -- -D warnings`, the full serialized
`cargo test -p ags --quiet -- --test-threads=1`, and all 34 clipboard/T3 Node tests.
The Rust suite includes 291 unit tests and 18 T3 lifecycle tests; seven existing
PSP integration tests remain ignored as configured. Real terminal interaction is
tested through a controlling pseudo-terminal, with Podman operations represented
by the existing protocol fixture. Original live Podman/T3 desktop smoke checks
remain tracked separately in the T3 integration feature.
