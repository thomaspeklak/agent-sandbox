# Glimpse in AGS

This document explains the **user-facing** AGS integration for Glimpse.

Use this if you want code running **inside the sandbox** to open **host-owned Glimpse windows**.

For the protocol/RFC details, see `docs/GLIMPSE_HOST_UI_BRIDGE.md`.

---

## What Glimpse means in AGS

With AGS host UI enabled:

- your agent still runs inside the sandbox
- the actual window is created on the **host**
- sandboxed code can keep using `glimpseui`
- AGS wires the sandbox to a host-side Glimpse bridge automatically

This is useful for:

- prompts and lightweight forms
- visual status windows
- host-owned webviews for sandbox-local apps
- packages that already use `glimpseui`

You should **not** need a browser inside the sandbox for the AGS-managed Glimpse path.

---

## What you need on the host

AGS does **not** ship the host-side Glimpse binaries itself.
You need working host binaries for:

- the host UI service (`glimpse_host_ui` / `glimpse-host-ui`)
- a renderer binary (`glimpse`) when using `renderer = "process"`

Exact paths depend on how you built or installed the Glimpse projects on your machine.

---

## Minimal config

Add a `[host_ui]` section to `~/.config/ags/config.toml`.

Example:

```toml
[host_ui]
enabled = true
binary = "/path/to/glimpse_host_ui"
renderer = "process"
renderer_bin = "/path/to/glimpse"
idle_timeout_ms = 0
log_level = "info"
```

### Field meanings

- `enabled`
  - turns on the AGS host UI sidecar
- `binary`
  - path or command for the host UI service
- `renderer`
  - `"process"` for real host windows
  - `"stub"` is mainly useful for testing, not normal interactive use
- `renderer_bin`
  - required when `renderer = "process"`
  - path to the actual Glimpse renderer binary
- `idle_timeout_ms`
  - host UI idle shutdown timeout
  - `0` disables idle shutdown and keeps the sidecar alive for the full AGS session
- `log_level`
  - host UI logging level

---

## How it works

At runtime AGS:

1. starts a per-session host UI service on the host
2. keeps that sidecar alive for the full AGS session by default
3. mounts its socket into the sandbox
4. points sandboxed `glimpseui` at AGS's bundled shim
5. lets the shim talk to the host UI service

In other words:

- your agent stays sandboxed
- the window lives on the host
- AGS handles the transport automatically

You should **not** need to set internal env vars like `GLIMPSE_BINARY_PATH` manually.

---

## Typical usage

Once `[host_ui]` is enabled and your image is current:

```bash
ags --agent pi
```

Then sandboxed code can use `glimpseui` normally.

If you changed AGS itself, rebuild the release binary and image first:

```bash
cargo build --release -p ags
ags update-image
```

Start a **fresh** AGS session after rebuilding.

---

## Quick verification inside the sandbox

Run:

```bash
ags --agent shell -- -lc 'echo "$AGS_HOST_UI_SOCK"; echo "$GLIMPSE_BINARY_PATH"; ls -l /opt/ags/glimpse-shim'
```

Expected:

- `AGS_HOST_UI_SOCK=/run/ags-host-ui/host-ui.sock`
- `GLIMPSE_BINARY_PATH=/opt/ags/glimpse-shim`
- `/opt/ags/glimpse-shim` exists

If those are true, AGS is exposing the host-UI-backed Glimpse path.

---

## Troubleshooting

### Glimpse falls back to Chromium / Chrome detection

Symptoms:

- logs mention `No Chromium or Chrome installation found`
- sandboxed code behaves like ordinary upstream `glimpseui` fallback logic

Usually this means one of these:

- your `ags` binary is stale
- your sandbox image is stale
- you are still in an old AGS session
- `[host_ui]` is disabled or misconfigured

Fix:

```bash
cargo build --release -p ags
ags update-image
ags doctor
```

Then start a new session and verify:

```bash
ags --agent shell -- -lc 'echo "$GLIMPSE_BINARY_PATH"; echo "$AGS_HOST_UI_SOCK"'
```

### No host window appears

Check:

- `[host_ui].enabled = true`
- `[host_ui].binary` points to a real executable
- `[host_ui].renderer = "process"` for normal usage
- `[host_ui].renderer_bin` points to a real executable

Run:

```bash
ags doctor
```

### SELinux alerts mentioning `pasta` / `code`

If you previously used an older build that relabeled bind mounts, your worktree may still have the wrong SELinux context.

Restore labels:

```bash
restorecon -RFv /home/$USER/code/agent-sandbox
```

If your repo lives elsewhere, run `restorecon` on that parent path instead.

---

## Related docs

- `docs/CONFIG.md` — config field reference
- `docs/COMMANDS.md` — runtime behavior and side effects
- `docs/TROUBLESHOOTING.md` — common failures and fixes
- `docs/GLIMPSE_HOST_UI_BRIDGE.md` — protocol and architecture details
