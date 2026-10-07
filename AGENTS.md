# Agent instructions

## Workflow

- This repository no longer uses Beads for issue tracking. Do not initialize `.beads/` or run Beads triage, issue-management, or sync commands for project work.
- Before finishing, check `git status`, validate changes, commit the relevant files, and push the branch.

## Code size policy

- Keep Rust implementation files at or below **500 lines**.
- Split files before they grow past the limit; do not keep adding to oversized modules.
- Keep tests out of the implementation file whenever possible.
  - Prefer `crates/ags/tests/` for integration coverage.
  - Prefer sibling `*_tests.rs` files for module-private coverage instead of inline `#[cfg(test)] mod tests` blocks.
- If you touch an oversized file, reduce or split it as part of the change.
