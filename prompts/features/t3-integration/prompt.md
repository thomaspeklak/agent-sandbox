# AGS / T3 Code Integration

## Planning status and source

The implementation plan was approved on 2026-10-08. Implementation and sandbox
checks are complete on `feat/t3-integration`; live Podman/desktop smoke verification
remains pending. These requirements reflect the final user decisions, which supersede
earlier assistant assumptions.

- Original OpenCode session: `ses_ee4012657ffep7BulAQEMgAgCg`.
- Session title: `Agent-sandbox support for t3code`.
- Repository: `/home/siyb/projects/agent-sandbox`.
- Session database: `/home/dev/.local/share/opencode/opencode.db`.
- The session was retrieved using read-only SQLite access, including its final
  assistant plan, user messages, and recorded question answers.

## Goal

Run T3 Code's server and supported coding providers inside an AGS-managed
repository environment, connected to an unchanged T3 desktop through SSH.
Retain AGS's existing isolation and shared provider-home behavior while making
repository worktrees directly usable both inside the sandbox and on the host.

## Final requirements

### Agent selection and installation

- Support the public launch interface `ags --agent t3`.
- Make T3 selectable in the Agent CLIs panel of `ags tools`.
- Make `ags update-agents` install or remove T3 according to the user's
  configuration.
- Install npm package `t3` through pnpm, like Pi, using the existing installer
  and verified runtime-generation policies.
- Preserve explicit user selections. Selecting T3 must not automatically enable
  its backing providers.
- Support T3-only configurations, including runtime inventory and mounts.

### Repository environments and SSH

- Use one environment/container per repository, shared by its worktrees.
- Use one saved T3 SSH connection per repository environment. Multiple
  environments must remain usable in the same T3 desktop UI.
- A T3 connection must automatically start or reuse AGS through a genuine
  host-side SSH bridge, even when the repository container is stopped.
- Run remote commands and forward loopback services inside the selected
  container. Desktop bootstrap scripts must not execute as host shell commands.
- Keep SSH transport separate from interactive trust and credential prompts.

### Worktrees, state, and provider integration

- Worktrees must exist at identical absolute host/container paths.
- Existing and newly T3-created worktrees must be directly usable by host Git
  commands and editors.
- Persist the parent directory needed for future T3-created worktrees, along
  with the repository and required external Git metadata.
- Preserve existing AGS isolation, shared provider homes, authentication,
  history, and applicable hooks and sandbox instructions.
- Do not redesign provider-home isolation.
- T3's own repository-scoped home/storage must not silently move provider
  authentication or history away from the existing `/home/dev` homes.
- Respect T3's provider permission selection instead of unconditionally
  injecting permission-bypass flags.

### Versions, lifecycle, and upgrades

- Exact desktop/runtime version mismatches must fail clearly.
- T3 bootstrap and pairing must not download replacement runtimes. Runtime
  installation and replacement belong to `update-agents`.
- Environments and active jobs must keep running after the final desktop/SSH
  connection disconnects, until explicitly stopped.
- Desktop teardown must not stop the AGS-owned server or restart an environment
  that was explicitly stopped through AGS.
- Explicit upgrades must preserve mounted T3 data, worktrees, and existing
  provider homes. The unmounted container writable layer may be replaced.
- Publishing a new runtime generation must not switch active environments.
- Provide explicit repository-scoped status, stop, and upgrade/recreate controls.

## Compatibility facts and initial target

The initial compatibility baseline is T3 Code v0.0.45:

- SSH bootstrap uses `$HOME/.t3` explicitly; `T3CODE_HOME` alone does not
  redirect its generated runtime and server-base paths.
- Bootstrap prepares an exact-version runtime before checking for server reuse.
- It expects `runtime/versions/<version>/t3` and `.install-complete`, with the
  marker containing the requested version.
- Launch, pairing, and teardown send shell scripts through SSH stdin.
- Pairing independently invokes the runtime.
- Forwarding targets the container's loopback service.
- The default worktree root is `<baseDir>/worktrees`, which is
  `$HOME/.t3/worktrees` for the SSH bootstrap's default base directory.
- The published npm package includes optional platform packages and bundled
  native dependencies. The root monorepo manifest is not the published CLI
  manifest and does not establish that the installed runtime is dependency-free.

The compatibility adapter must validate supported command/script forms and
versions before execution. Unknown forms must fail actionably before reaching
installer or download paths. Read-only runtime mounts are necessary but do not
replace that preflight, because upstream installation-lock retries can delay
failure.

## Deferred follow-up

General Node worktree-version discovery is deferred to a GitHub follow-up. The
integration may use AGS's existing fixed-image Node bootstrap for the npm T3
launcher where required. Follow-up: https://github.com/thomaspeklak/agent-sandbox/issues/23.

## Repository constraints

- Follow `AGENTS.md`; do not initialize or use Beads for project work.
- Keep Rust implementation files at or below 500 lines. Split near-limit files
  before extending them, and reduce any oversized implementation file touched.
- Prefer integration tests in `crates/ags/tests/` and sibling `*_tests.rs` files
  for module-private coverage.
- Validate behavior according to `CONTRIBUTING.md`, including formatting,
  Clippy, AGS tests, relevant Node tests, and live Podman/T3 smoke tests.
- Keep user-facing documentation and configuration examples aligned with the
  implemented behavior.
- Before finishing implementation work, inspect status, validate, commit the
  relevant files, and push the branch.
