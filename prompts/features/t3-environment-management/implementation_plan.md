# Approved implementation plan

The user requested implementation of all changes discussed in this session.

1. Discover registrations directly from the host registry, independent of the
   working directory. Keep registration ownership/identity validation and isolate
   invalid entries so global status can report healthy environments alongside
   errors. Query owner status with a bounded timeout and inspect container state
   without starting or repairing environments.
2. Add an optional environment identifier and the explicit `--all` switch to
   public lifecycle commands. Use the stable managed SSH alias as the displayed
   identifier; also accept the full repository identity. Keep `--repository` for
   explicit path targeting. Reject conflicting selectors and preserve internal
   owner/proxy registration targeting.
3. Make global status a JSON array and targeted status a JSON object, retaining
   existing detail fields. Include owner, server, container, runtime, identity,
   and availability/error information for every discovered registration.
4. Implement a terminal checkbox selector using the existing terminal UI
   dependencies. List working environments, repository paths, aliases, and
   runtime versions with no boxes checked. Support navigation, Space, Enter,
   Escape, and Ctrl-C; restore the terminal before executing any action.
5. Run global stop/upgrade only for explicitly selected working environments, or
   every working environment with `--all`. Non-interactive global invocation
   requires an explicit selector or `--all`. Run sequentially, preserve existing
   lifecycle semantics, report outcomes, and aggregate failures.
6. Add parser/help, inventory/lifecycle, and real pseudo-terminal selection tests.
   Update lifecycle documentation and completions. Run formatting, Clippy, the
   serialized Rust suite, relevant Node tests, and diff checks; commit and push.

## Recovery correction from live smoke feedback

The working-only upgrade filter above is superseded: global upgrade must offer
all valid registrations, including stopped/unavailable instances, with state and
diagnostics in the unchecked list. Explicit selection opts into startup of a
stopped instance. Global stop remains working-only. Recheck the displayed state
before acting so a later stop is not reversed and newly started work is not
interrupted by a stale selection. Apply the same eligibility to `--all`.
