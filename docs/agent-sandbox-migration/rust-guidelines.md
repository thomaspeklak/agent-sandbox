# Rust Guidelines for Agent Sandbox Migration

## Code structure

- Keep production Rust files **under 400 LOC**.
- If a file grows too much, split by responsibility (parser, planner, executor, adapters, etc.).
- Prefer small modules with clear public APIs over one large file.

## Tests location

- Do **not** put tests inside production files.
- Avoid inline `#[cfg(test)] mod tests { ... }` in code files.
- Put tests in separate files, preferably under `tests/`.
- If module-focused tests are needed, keep them in separate dedicated test files, not mixed into implementation files.

## Command cadence (fast feedback first)

Precondition: these commands apply once a Rust workspace exists (`Cargo.toml` present).
For pre-scaffold documentation/planning tasks, record the missing workspace as expected.

Run this way while implementing:

1. Run `cargo check` often (after each meaningful change).
2. Run `cargo fmt --all` before finishing.
3. Run `cargo clippy --all-targets --all-features` before finishing.
4. Run tests **last**: `cargo test --all-targets --all-features`.

Reasoning:

- `cargo check` is fastest and catches most compile/type issues early.
- `fmt` + `clippy` clean up style and lint issues before expensive test builds.
- tests require compilation and are slower, so run them once near the end.

## Definition of done (per task)

- Task implemented with minimal scope.
- `cargo check` passes.
- `cargo fmt --all` applied.
- `cargo clippy --all-targets --all-features` passes.
- `cargo test --all-targets --all-features` passes (run last).
- Relevant migration markdown checkboxes updated.
