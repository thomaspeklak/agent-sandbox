# Startup prepare hooks (v1)

AGS runs optional **host executables**, before any container creation, to prepare a launch.
Only `prepare` is implemented. `started` and `finished` are reserved future events.
There is no hook-to-hook dependency ordering, interactive hook prompting, bidirectional
protocol, agent-settings merge, security override, retry, or AGS-managed external service
lifecycle. This is not a container sandbox for hook code.

## Configure

Add repeatable declarations to your global AGS config or `.ags/config.toml`:

```toml
[[prepare_hook]]
name = "development-services"
executable = "./prepare-services" # relative to this config file, NOT the working directory
args = ["--profile", "development"] # argv, not a shell command string
# timeout_seconds = 30             # 1..300 seconds
```

Global declarations followed by project declarations **accumulate**, never replace.
Use unique names to select a hook with `ags hooks test`; launch itself permits repeated
names. At most 64 declarations are accepted per launch. Entrypoints must be executable
regular files, at most 64 MiB. Both native binaries and executable scripts support arguments.
For a script use a shebang and executable permission (`chmod +x prepare-services`).

## Trust is host-user execution approval

Before **any** hook runs, AGS requires approval of every active declaration. Approval
binds SHA-256 of the full declaration (including argv, name and timeout), entrypoint
bytes, and project scope for project hooks. New or changed hashes require a fresh AGS-owned
terminal prompt. The prompt shows the executable, argv, scope, hash, and authority granted.
No real hook runs merely to calculate a hash. AGS rechecks approval immediately before
execution, and executes a private copy of the approved entrypoint bytes, not a mutable
original file. Changes after that check cannot change the tracked bytes being executed.

Approvals are stored **outside the repository** in `~/.local/state/ags-hook-trust/`,
with owner-only directory/file permissions (0700/0600); symlink tokens and insecure
permissions are refused. Remove a hash-named file to revoke that approval. AGS refuses
approval storage located inside the active project. Global approvals apply across projects;
project approvals are scoped to the canonical project path, including when `.ags/config.toml`
is explicitly selected as the primary config.

Repository-overlay approval is separate. If a discovered project overlay declares hooks
but overlay trust is absent or denied, AGS refuses rather than silently dropping those hooks.
Overlays without hook declarations retain the existing skip-on-denial convention. Explicit
same-file primary-config selection does not double-load or separately approve that overlay;
its hooks still need hash approval. Noninteractive new/changed hooks fail with instructions
to rerun interactively with the same config, or use `ags hooks test NAME --config PATH`.

**This is entrypoint approval, NOT transitive code integrity.** Interpreters, dynamically
linked libraries, imported/sourced code, scripts passed as interpreter arguments, configuration
files, and other resources loaded by approved code are **not content tracked**. For example,
`/usr/bin/python3` plus `helper.py` tracks Python and argv, not `helper.py` contents. Declare
the directly executable script itself when changes to that script should require approval.
An approved hook has the host user's authority and can independently read credentials,
modify files, start services, or perform network operations.

The private execution copy retains the entrypoint's basename, but its full `argv[0]`/script
`$0` and directory change. `$ORIGIN` libraries and entrypoint-relative resource discovery
may therefore fail. Use context/explicit absolute resource paths; AGS fails rather than
falling back to unapproved original bytes. The temporary filesystem must permit execution.

## JSON protocol

Each hook gets the **same** UTF-8 JSON context on stdin, then EOF:

```json
{"version":1,"event":"prepare","launch_id":"ags-123-unique","project":"/work/project","workdir":"/work/project/subdir","agent":"pi"}
```

`project` is the Git repository root (or working directory outside Git), `workdir` is
canonical, and `launch_id` identifies the prepare phase, not the container name. No
credentials, resolved secret values, or other hooks' responses are included. Hooks run
with `workdir` as cwd and a scrubbed inherited environment: only PATH, HOME, USER, LOGNAME,
XDG_RUNTIME_DIR and DBUS_SESSION_BUS_ADDRESS are retained. This environment scrub is not
a restriction on the approved program's own ability to load host credentials.

Stdout must contain **one** JSON response. Unknown fields, duplicate JSON object keys,
invalid types/version, trailing non-whitespace, nonzero exit and invalid contributions fail
closed. Stderr is bounded diagnostic output (terminal-control characters are escaped).
Never print credentials to diagnostics. For example:

```json
{
  "version": 1,
  "env": {
    "SERVICE_URL": "http://host.containers.internal:8100",
    "SERVICE_TOKEN": {"op": "op://Development/service/password"}
  },
  "files": [
    {"destination": "/tmp/development-client.json", "content": "{\"timeout\":10}\n"}
  ],
  "mounts": [
    {"source": "/absolute/host/generated-data", "destination": "/workspace/service-data", "mode": "ro"}
  ]
}
```

All contribution sections are optional. Environment literals and secret references share
one keyspace. Values must be single-line, NUL-free UTF-8 for the existing environment-file
transport. References use `op://vault/item/[section/]field`; interpolation and arbitrary
commands are not supported. Bind sources must be existing regular files/directories, absolute,
and free of NUL/newlines/colon. AGS does not create these external sources. Generated files
are UTF-8 response content, staged as private 0600 files and mounted read-only. There is no
hook-specified executable mode or secret-reference expansion into file contents in v1.

Destinations must be normalized absolute paths beneath `/tmp`, `/workspace`, or unreserved
`/home/dev` data paths. Agent config/home settings, AGS runtime/security resources, image
executables, hidden home-root settings, `/tmp/ags-*` runtime resources, protected caches
and their aliases/ancestors cannot be replaced. Reserved
runtime/guard/host-auth and agent-configuration redirector environment keys (including
`XDG_CONFIG_HOME`, `OPENCODE_CONFIG_DIR` and Gemini system-settings paths) are rejected;
API-key variables such as `OPENCODE_API_KEY` remain allowed. See the output schema and
validator for the precise keyspace. Hook mounts exposing AGS host cache/trust storage are refused,
including hook approvals and the repository-overlay trust file at
`$XDG_CONFIG_HOME/ags/trusted-repo-overlays.txt` (normally `~/.config/ags/`). Canonical
aliases and ancestor directories are protected even before that trust file is created.
A destination nested in an existing workdir/config/managed mount is an ambiguous overlap,
not an implicit overlay: choose an independent data destination instead.

## Deterministic resolution and deadlines

Precedence for known keys/destinations is **defaults < global hooks < project hooks < explicit
CLI options**. Within a scope, later declaration wins regardless of completion order.
Later entries within a response's file/mount array win; mounts are applied after files
within that response. Exact file/mount destination collisions share one keyspace and replace
by precedence. Distinct ancestor/descendant destinations are rejected, including effective
plan/CLI mount overlaps. Existing launch-plan protected-cache validation still applies.

Default concurrency is 4, per-hook timeout 30 seconds, overall execution/snapshot phase
60 seconds, stdout limit 1 MiB per hook, stderr limit 64 KiB per hook. Queued execution
uses the same phase deadline. Hooks see no other results. A failure or SIGINT/SIGTERM/SIGHUP
cancels peers; AGS kills members of its hook process groups, including normal child descendants,
even after a successful entrypoint exit, and reaps its direct children. Reaping descendant
zombies depends on the host's init/reaper; AGS is not a process-wide subreaper. No automatic
hook retries. Deliberately daemonized or re-grouped external processes cannot be contained
as a sandbox would contain them; approved hooks own their external resources and side effects,
including failure cleanup.

Only **surviving known-key** 1Password references are centrally deduplicated, then resolved
in one `op inject` template batch at the final launch handoff. AGS checks installed `op inject`
capabilities with credential-free help first; install/update op if unsupported. Hooks do not
receive resolved values. They must not resolve secrets themselves if central pruning is desired.
Values are not stored in config, approval tokens, caches or logs; they use the existing private
0600 runtime environment-file transport, removed on success/handled launch failure. This is
short-lived protected plaintext transport, not a promise of zero disk exposure: uncatchable
termination/crash can retain that existing transport file. Generated files/execution snapshots
are AGS-owned temporary files, removed on normal/handled cancellation/failure returns.

Compatibility boundary: explicit `--op-secret-set` still uses opaque Secure Note item
payloads and the existing final-process bootstrap. Its field names are not known during
planning, so it retains final-process overwrite precedence but **cannot** prune same-key
hook lookups in advance. A hook lookup may still occur/fail even if an opaque CLI item later
replaces it. Use `--env KEY=VALUE` for a known-key CLI override that eliminates a reference.

### Lockdown

Trusted hooks may execute even with `--lockdown` (they run on the host, outside the sandbox).
After precedence, AGS allows literal env and private generated data files, but rejects surviving
hook op references and host bind mounts before secret lookup/sidecar/container creation. CLI
`--env` can eliminate a losing reference; `--add-dir` retains its existing explicit semantics.
These are AGS-managed injection restrictions, **not** a promise that approved host code
cannot itself access host files or secrets. AGS runtime/security destinations remain protected.

## Discover and test

```sh
ags hooks describe prepare
ags hooks schema prepare input
ags hooks schema prepare output
ags hooks validate response.json # inert; '-' reads stdin
ags hooks test development-services --config /path/to/config.toml --agent shell
ags hooks test development-services --context prepare-context.json --config /path/to/config.toml
```

Schemas are published as [prepare input](schemas/prepare-input.schema.json) and
[prepare output](schemas/prepare-output.schema.json) in `docs/schemas/`. JSON Schema
covers structural/static rules; the validator additionally enforces protected paths/keyspace,
byte bounds and contribution overlaps. Host existence and effective launch-plan checks are
contextual and cannot all be represented by JSON Schema.

**`test` EXECUTES HOST CODE and is not a side-effect-free dry run.** It checks trust,
executes only the named hook, validates its response/materialization, and prints a redacted
contribution summary (keys/destinations only). It does not run Podman, resolve op values,
start other hooks, or prove compatibility with every effective runtime mount. With no
`--context`, context uses the current directory (or `--workdir PATH`) and selected `--agent`
(default shell). An explicit JSON context cannot be combined with agent/workdir overrides.
Trust prompting is AGS-owned; hooks themselves must never wait for interactive stdin.

## Example: Pi intercom namespace

[ags-hook-pi-intercom-namespace](https://github.com/thomaspeklak/ags-hook-pi-intercom-namespace)
is a standalone MIT-licensed prepare hook that sets `PI_INTERCOM_SCOPE_ID` for `pi`
and `shell` launches. Its default `project` mode uses the repository name (for example,
`agent-sandbox`) to group sessions across worktrees. Optional `worktree` mode adds the
checkout folder name when multiple actual checkouts exist. Explicit `global` mode emits
an empty scope to join other unscoped sessions; it does not bridge to scoped sessions.
For supported non-`pi`/`shell` agents, the hook is a no-op and leaves any inherited scope
unchanged. Names are routing conveniences, not authentication or guaranteed isolation;
repository or checkout names can collide.

AGS does not install or enable this hook. Install it separately, review its executable
and configuration, declare it as a prepare hook, and grant normal AGS host-execution
approval. See the linked repository for setup, configuration, naming limitations, and
the MIT license. Setting a scope alone does not establish intercom broker reachability.
