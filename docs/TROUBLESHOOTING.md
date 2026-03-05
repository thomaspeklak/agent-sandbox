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
