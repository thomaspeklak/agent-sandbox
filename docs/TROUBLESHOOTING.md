# Troubleshooting

Start here:

```bash
ags doctor
```

Most issues are visible in doctor output.

---

## `error: required mount source missing`

A required mount path in config does not exist.

### Fix

- Create the path on host, or
- mark mount as `optional = true`, or
- set `create = true` (for paths safe to auto-create)

---

## Container image missing or outdated

Symptoms:

- image-not-found errors
- old runtime dependencies

### Fix

```bash
ags update
```

Then if needed:

```bash
ags update-agents
```

---

## `br` / `bv` missing inside container

If Beads commands are missing or stale in sandbox.

### Fix

```bash
ags update
ags --agent shell -- -lc 'br --version && bv --version'
```

`ags update` refreshes from upstream releases:

- `beads_rust` (`br`): https://github.com/Dicklesworthstone/beads_rust/releases
- `beads_viewer` (`bv`): https://github.com/Dicklesworthstone/beads_viewer/releases

---

## `psql` missing inside container

If `psql` is not found in sandbox shell.

### Fix

```bash
ags update
ags --agent shell -- -lc 'psql --version'
```

---

## `tmux` missing when using `--tmux`

Symptoms:

- `ags --agent <name> --tmux` fails with `tmux: not found`
- or AGS prints a message telling you to rebuild the sandbox image

Cause:

- your existing sandbox image was built before tmux support was added

### Fix

```bash
ags update
ags --agent shell -- -lc 'tmux -V && test -f ~/.tmux.conf'
```

---

## `missing or unsuitable terminal: xterm-kitty` in tmux

Symptoms:

- running `tmux` inside the sandbox fails with `missing or unsuitable terminal: xterm-kitty`

Cause:

- your host terminal exports `TERM=xterm-kitty`
- the sandbox image needs kitty terminfo entries available for tmux to start cleanly

### Fix

Rebuild the image so the sandbox includes `kitty-terminfo`:

```bash
ags update
ags --agent shell -- -lc 'echo "$TERM" && tmux -V'
```

Temporary workaround if you need it before rebuilding:

```bash
TERM=xterm-256color ags --agent shell
```

---

## Cannot reach host service from agent (`localhost` confusion)

Symptoms:

- Service works on host but fails from inside agent/shell
- `curl http://localhost:<port>` fails in sandbox

Cause:

- Agent runs inside container, so `localhost` points to container itself.

### Fix

Use host gateway name instead:

```bash
curl http://host.containers.internal:<port>
```

Example:

```bash
ags --agent shell -- -lc 'curl http://host.containers.internal:3000/health'
```

You can verify runtime hint env vars too:

```bash
ags --agent shell -- -lc 'echo "$AGS_HOST_SERVICES_HOST" && echo "$AGS_HOST_SERVICES_HINT"'
```

---

## Agent CLI missing inside container

If command like `pi`, `codex`, `gemini`, `opencode`, or `claude` is missing/old.

### Fix

```bash
ags update-agents
```

---

## SSH problems (git auth/signing)

Symptoms:

- cannot push
- signing fails
- keys not loaded

### Fix

1. Re-run setup:
   ```bash
   ags setup
   ```
2. Confirm public keys are added in GitHub:
   - auth key as SSH key
   - signing key as SSH signing key
3. Re-run:
   ```bash
   ags doctor
   ```

If keys are passphrase protected, `ssh-add` may prompt interactively.

---

## Secret not available inside container

Symptoms:

- tool auth failures
- missing token env vars

### Check

- Is `[[secret]]` / `[[tool.secret]]` configured correctly?
- Is source env var actually set and non-empty?
- If using `secret_store`, does `secret-tool lookup ...` return a value?

### Fix

- Re-run `ags setup` to re-enter secrets (if using interactive keyring flow)
- Export env vars before launching `ags`

---

## Browser mode fails

Symptoms:

- `--browser` exits early
- debug endpoint not reachable

### Check

- `[browser].enabled = true`
- `[browser].command` exists and executable
- `[browser].profile_dir` is valid
- `[browser].debug_port` is non-zero and free

### Fix

```bash
ags doctor
ags --agent pi --browser
```

If needed, try a different debug port.

---

## Config parse/validation errors

Symptoms:

- startup fails with validation message

### Fix

- Compare your file with `config/config.example.toml`
- Verify enums are valid:
  - mount mode: `ro|rw`
  - mount kind: `dir|file`
  - mount when: `always|browser`
- Verify required strings are non-empty

---

## `ags` command not found

If installed from source but not in PATH.

### Fix options

Run via cargo:

```bash
cargo run -p ags -- --agent pi
```

Or self-link:

```bash
cargo run -p ags -- install --link-self
```

Ensure `~/.local/bin` is in PATH.

---

## Alias/wrapper commands not found

If you ran `ags create-aliases` but short names are missing.

### Check

- Wrappers mode writes to `~/.local/bin`
- Aliases mode updates shell rc and requires shell reload

### Fix

```bash
ags create-aliases --mode both
exec $SHELL
```

---

## Podman runtime issues

Symptoms:

- `podman` command failures
- permission/network oddities

### Check

- Podman installed and working rootless
- user session has required Podman setup

Run a quick check:

```bash
podman info
```

Then rerun:

```bash
ags doctor
```
