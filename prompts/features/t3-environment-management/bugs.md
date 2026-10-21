# Global upgrade recovery gap

- [x] Offer valid stopped and recoverable unavailable registrations in global
  upgrade, including after configuration changes cause startup to fail. Keep
  global stop restricted to working instances, show state in unchecked lists,
  and guard against state changes while the picker is open.

Reported sequence: `ags --agent t3` fails with configuration-change guidance,
then `ags t3 upgrade` reports no working environments rather than offering the
registered environment for recreation. A scoped upgrade is a current workaround.

The reported sequence reproduced the no-choice result before the fix. A real
pseudo-terminal regression now recreates the stopped environment through global
upgrade and verifies retained history/identity. Additional coverage checks
ownerless recovery, invalid-entry exclusion, stopped-list cancellation, mixed
running/stopped bulk upgrade, and a stopped selection starting while the picker
is open. State transitions still require reopening the picker.

Validation passed: locked fetch, formatting, all-target Clippy, the full
serialized Rust suite (293 unit tests and 28 T3 lifecycle tests), all 34 Node
tests, and diff checks. Live host recovery remains to be retried with the updated
binary or the existing scoped command.
