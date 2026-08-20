# 1Password Secure Note environment sets

AGS can inject environment variables from a 1Password item for one explicit run:

```console
ags --agent pi -1 'ExampleVault/readonly-database'
```

`-1` is short for repeatable `--op-secret-set VAULT/ITEM`:

```console
ags --agent pi \
  -1 'Employee/common env vars' \
  -1 'ExampleVault/readonly-database'
```

## Supported item type

**Only 1Password items whose category is `SECURE_NOTE` are supported.** Login, Password, API Credential, Identity, and every other item category fail the launch.

A Secure Note acts as an environment-variable set. Every `fields[]` entry with a present string `value` is injected with its exact `label` as the environment-variable name. The labels must therefore already be valid environment names, for example:

- `PGHOST`
- `PGPORT`
- `PGUSER`
- `PGPASSWORD`
- `PGDATABASE`

Empty strings are valid values. Fields without a value are ignored. Invalid present labels or values fail the launch instead of being omitted. Duplicate labels use last-wins order: later fields win within an item, and later `-1` arguments win across items.

There is no `config.toml` key, field mapping, allowlist, preset, or automatic activation for this feature.

## Runtime and security model

- `op item get ITEM --vault VAULT --format=json --reveal` runs on the **host**, using normal interactive 1Password authentication.
- AGS does not mount `op` into the container or forward `OP_*` authentication variables.
- Values are sent through sealed anonymous file descriptors, not argv, Podman configuration, environment files, or regular files.
- The long-lived AGS host process does not deserialize or copy the item JSON; the final container bootstrap parses it immediately before the agent starts.
- Only the final agent process tree receives the environment. This includes `psql`, MCP servers, subagents, tmux panes, and post-agent tmux shells.
- `op` and the final bootstrap necessarily see plaintext briefly. The final agent environment is inspectable by authorized same-user/root processes through `/proc/<pid>/environ`.
- Vault and item names/IDs are metadata visible in the host `op` command line.

Use a dedicated least-privilege/readonly database role. Rotate or revoke it independently of other credentials.

## Requirements and limits

- Local Podman with `--preserve-fds` support is required.
- Remote Podman connections are rejected for `-1` runs because anonymous descriptors cannot safely cross the client/server boundary.
- If Podman retries a failed legacy-network launch, AGS must retrieve fresh one-shot descriptors and therefore runs `op` again; normal 1Password authentication may prompt again.
- `--lockdown` is incompatible with `-1` / `--op-secret-set`.

## Safe smoke test

Check required names and database connectivity without printing a password or dumping the environment:

```console
ags --agent shell -1 'ExampleVault/readonly-database' -- \
  -lc 'test -n "${PGHOST-}" && test -n "${PGUSER-}" && psql -c "select 1" >/dev/null'
```
