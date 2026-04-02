# Glimpse Host UI Bridge for AGS (MVP RFC)

Status: Accepted for implementation

Owners:
- `agent-sandbox` — session lifecycle, socket mount, env discovery, operator UX
- `../glimpse` — client transport selection and API compatibility
- `../glimpse_rust` — host-owned UI service and renderer adapter

Related issues:
- `agent-sandbox-mp3` — epic
- `agent-sandbox-mp3.1` — this RFC

---

## 1. Goal

Allow code running **inside the AGS sandbox** to open **host-owned Glimpse webviews** without mounting the host display server, session bus, or other broad desktop capabilities into the container.

The same high-level Glimpse API should work in both places:

- **outside AGS**: direct renderer/backend
- **inside AGS**: socket-backed host bridge

This RFC defines the MVP architecture, runtime rules, protocol, and security boundaries.

---

## 2. Non-goals

This RFC does **not** try to provide:

- a permanent system daemon
- generic host desktop automation
- arbitrary host file access through the webview bridge
- raw host Wayland/X11/session bus mounts into the sandbox
- a broad “run anything on the host” RPC mechanism

This is a **narrow UI bridge** for Glimpse-style windows and prompts.

---

## 3. Chosen architecture

### 3.1 Layers

We split the system into 3 layers:

1. **Client API (`../glimpse`)**
   - keeps `open()` / `prompt()` for callers
   - chooses transport at runtime
   - direct mode outside sandbox, socket mode inside AGS

2. **Host UI service (`../glimpse_rust`)**
   - host-owned Unix socket server
   - owns sessions, windows, and request/event routing
   - renderer-agnostic core with a macOS Glimpse adapter

3. **AGS integration (`agent-sandbox`)**
   - starts the host UI service per AGS run
   - mounts only the dedicated runtime dir/socket into the sandbox
   - injects discovery env vars
   - stops the service when the AGS session ends

### 3.2 Why this architecture

This gives us:

- a single API for callers
- no host display/session primitive exposure inside the sandbox
- host-owned lifecycle for windows
- a protocol that can later move behind a longer-lived daemon

---

## 4. Runtime backend selection

### 4.1 Rule

`../glimpse` must support these transport modes:

- `auto` (default)
- `direct`
- `socket`
- `off`

### 4.2 Default resolution

When transport is `auto`:

1. if `AGS_SANDBOX=1` **and** `AGS_HOST_UI_SOCK` is set, use **socket** mode
2. otherwise use **direct** mode

### 4.3 Explicit override

The client library may expose an explicit override for testing/dev, but AGS only guarantees the env-based contract above.

### 4.4 Important policy decision

**Sandbox detection lives above the renderer binary.**

Reason:
- the renderer should only know how to render windows
- transport and deployment topology are host/application concerns
- putting sandbox detection in the renderer would couple renderer internals to AGS runtime policy

The renderer may support different operating modes, but **the host/service layer chooses the mode**.

---

## 5. Process and lifecycle model

### 5.1 MVP lifecycle

For MVP, AGS starts a **session-scoped host UI service** for each AGS run:

1. AGS starts host UI service on the host
2. AGS waits for readiness
3. AGS mounts the runtime dir/socket into the container
4. sandboxed code connects to the socket through `../glimpse`
5. when the AGS session ends, AGS terminates the service

### 5.2 No lingering process rule

The service must not become an unbounded background process.

MVP shutdown conditions:
- AGS process exits or tears down the session
- optional operator-configured idle timeout, if explicitly enabled

Default behavior should keep the sidecar alive for the full AGS session.

### 5.3 Future compatibility

The protocol must remain usable if we later switch to:

- one host service shared by multiple AGS sessions
- a daemon with client refcounting / leases

That future must **not** require changing the sandbox client API.

---

## 6. Runtime paths and environment contract

### 6.1 Host runtime dir

AGS creates a per-session runtime dir on the host, similar to `auth_proxy` and PSP patterns.

Suggested shape:

- host runtime dir: `${XDG_RUNTIME_DIR:-/tmp}/ags-host-ui-<pid-or-session>`
- host socket path: `<runtime-dir>/host-ui.sock`

### 6.2 Container mount

AGS mounts the runtime dir into the container at:

- `/run/ags-host-ui`

Socket path inside container:

- `/run/ags-host-ui/host-ui.sock`

### 6.3 Environment variables injected by AGS

AGS must inject:

- `AGS_HOST_UI_SOCK=/run/ags-host-ui/host-ui.sock`
- `AGS_HOST_UI_PROTOCOL=1`
- `AGS_HOST_UI_TRANSPORT=socket`

Optional but recommended:

- `AGS_HOST_UI_SESSION_ID=<stable-ags-session-id>`
- `AGS_HOST_UI_HINT=[ags] Host UI available through mounted socket; host owns Glimpse windows`

### 6.4 Prompt hinting

For interactive agents, AGS may also inject a short prompt/context hint, similar to the existing host-service hinting pattern.

---

## 7. API surface exposed through the bridge

The bridge owns a **small** UI API:

- `hello`
- `open`
- `prompt`
- `update`
- `close`

That is enough for MVP.

Window content may come from either:
- inline HTML
- a host-reachable HTTP(S) URL generated by AGS relay infrastructure for sandbox-local app servers

Ownership boundary for served-app URLs:
- packages construct their normal localhost app URL
- `glimpseui` resolves sandbox-local localhost URLs through the AGS relay when needed
- AGS owns the relay itself

See `docs/GLIMPSE_SANDBOX_APP_ORIGIN_RELAY.md` for the served-app extension.

### 7.1 `open`

Open a persistent window and return a `window_id`.

### 7.2 `prompt`

Open a one-shot window and wait until:
- first `message` from the page, or
- close without a message

Then return:
- the message payload, or
- `null` on close/cancel

### 7.3 `update`

Apply one or more changes to an existing window.

Supported MVP patch operations:
- replace HTML
- evaluate JS in the page
- show hidden window
- toggle follow-cursor

### 7.4 `close`

Close a persistent window.

---

## 8. What the bridge does **not** expose in MVP

The socket bridge must **not** expose these operations initially:

- host file loads by arbitrary path
- navigation to arbitrary host file URLs
- generic command execution on the host
- host OS dialogs outside the defined Glimpse window flow
- direct access to display server/session bus primitives

This is deliberate scope control.

---

## 9. Protocol transport

### 9.1 Transport type

The protocol is **newline-delimited JSON over a Unix domain socket**.

Why:
- already matches AGS sidecar patterns
- easy to debug
- easy to test
- works for request/response and async events

### 9.2 Connection model

Each sandbox client process opens a long-lived connection to the host UI service.

One connection may multiplex multiple requests/windows using request IDs and window IDs.

### 9.3 Versioning

All messages carry a protocol version field.

MVP protocol version:
- `v = 1`

Clients must send `hello` before other methods.

If the version cannot be negotiated, the server returns `version_mismatch` and closes the connection.

---

## 10. Protocol message envelope

All messages use one JSON object per line.

### 10.1 Client → server request

```json
{
  "v": 1,
  "kind": "request",
  "id": "req_123",
  "method": "open",
  "params": { ... }
}
```

### 10.2 Server → client response

```json
{
  "v": 1,
  "kind": "response",
  "id": "req_123",
  "ok": true,
  "result": { ... }
}
```

Error response:

```json
{
  "v": 1,
  "kind": "response",
  "id": "req_123",
  "ok": false,
  "error": {
    "code": "payload_too_large",
    "message": "html exceeds limit",
    "details": { "limit_bytes": 524288 }
  }
}
```

### 10.3 Server → client event

```json
{
  "v": 1,
  "kind": "event",
  "event": "window.message",
  "window_id": "win_123",
  "data": { ... }
}
```

---

## 11. Required methods

## 11.1 `hello`

Purpose:
- negotiate protocol version
- identify client
- return capabilities

Request:

```json
{
  "v": 1,
  "kind": "request",
  "id": "req_1",
  "method": "hello",
  "params": {
    "client_name": "glimpseui",
    "client_version": "0.x",
    "protocol_min": 1,
    "protocol_max": 1,
    "session_id": "optional-ags-session-id"
  }
}
```

Response:

```json
{
  "v": 1,
  "kind": "response",
  "id": "req_1",
  "ok": true,
  "result": {
    "protocol_version": 1,
    "server_name": "glimpse_host_ui",
    "server_version": "0.x",
    "capabilities": {
      "prompt": true,
      "follow_cursor": true,
      "platform": "darwin"
    }
  }
}
```

## 11.2 `open`

Request params:

```json
{
  "source": {
    "kind": "html",
    "html": "<html>...</html>"
  },
  "options": {
    "width": 400,
    "height": 300,
    "title": "My App",
    "x": 10,
    "y": 20,
    "frameless": false,
    "floating": false,
    "transparent": false,
    "click_through": false,
    "follow_cursor": false,
    "follow_mode": "snap",
    "cursor_anchor": "top-right",
    "cursor_offset": { "x": 20, "y": -20 },
    "hidden": false,
    "auto_close": false
  }
}
```

Served-app variant:

```json
{
  "source": {
    "kind": "url",
    "url": "http://127.0.0.1:43125/"
  },
  "options": {
    "width": 960,
    "height": 720,
    "title": "Interview"
  }
}
```

Successful response:

```json
{
  "v": 1,
  "kind": "response",
  "id": "req_2",
  "ok": true,
  "result": {
    "window_id": "win_123"
  }
}
```

After `open`, the client receives async events for that `window_id`.

## 11.3 `prompt`

Same params as `open`, plus optional timeout:

```json
{
  "source": {
    "kind": "html",
    "html": "<html>...</html>"
  },
  "options": {
    "width": 340,
    "height": 180,
    "title": "Confirm",
    "timeout_ms": 30000
  }
}
```

Behavior:
- service opens an internal window
- waits for first `message` or close
- closes window automatically
- returns final result synchronously

Success response:

```json
{
  "v": 1,
  "kind": "response",
  "id": "req_3",
  "ok": true,
  "result": {
    "value": { "ok": true }
  }
}
```

Cancel/close response:

```json
{
  "v": 1,
  "kind": "response",
  "id": "req_3",
  "ok": true,
  "result": {
    "value": null
  }
}
```

## 11.4 `update`

Request params:

```json
{
  "window_id": "win_123",
  "patch": {
    "html": "<html>...</html>",
    "js": "document.title = 'Updated'",
    "show": { "title": "Results" },
    "follow_cursor": {
      "enabled": true,
      "anchor": "top-right",
      "mode": "spring"
    }
  }
}
```

Notes:
- all patch fields are optional
- server applies provided fields in a defined order: `html` → `js` → `show` → `follow_cursor`
- unknown fields are rejected

## 11.5 `close`

Request:

```json
{
  "window_id": "win_123"
}
```

Response indicates whether close was accepted.

---

## 12. Required events

The service sends these events for persistent windows:

### 12.1 `window.ready`

```json
{
  "v": 1,
  "kind": "event",
  "event": "window.ready",
  "window_id": "win_123",
  "data": {
    "screen": { "width": 2560, "height": 1440 },
    "screens": [],
    "appearance": { "dark_mode": true },
    "cursor": { "x": 100, "y": 100 },
    "cursor_tip": null
  }
}
```

### 12.2 `window.message`

```json
{
  "v": 1,
  "kind": "event",
  "event": "window.message",
  "window_id": "win_123",
  "data": {
    "action": "submit",
    "value": 42
  }
}
```

### 12.3 `window.closed`

```json
{
  "v": 1,
  "kind": "event",
  "event": "window.closed",
  "window_id": "win_123",
  "data": {}
}
```

### 12.4 `window.error`

```json
{
  "v": 1,
  "kind": "event",
  "event": "window.error",
  "window_id": "win_123",
  "data": {
    "code": "renderer_error",
    "message": "renderer exited unexpectedly"
  }
}
```

---

## 13. Payload limits and resource bounds

MVP server-side minimum enforcement:

- max message line size: **1 MiB**
- max HTML payload: **512 KiB UTF-8**
- max JS patch payload: **128 KiB UTF-8**
- max title length: **1024 bytes**
- max window count per client connection: **16**
- max concurrent prompt requests per connection: **4**

If a limit is exceeded, return a structured error and do not partially apply the request.

Reasoning:
- protects host service from unbounded memory growth
- keeps protocol predictable
- still large enough for realistic single-file HTML UIs

---

## 14. Error model

Structured error codes for MVP:

- `version_mismatch`
- `invalid_request`
- `unknown_method`
- `payload_too_large`
- `window_not_found`
- `unsupported_option`
- `unsupported_platform`
- `renderer_unavailable`
- `timeout`
- `internal_error`

Rules:
- request failures use `response.ok = false`
- asynchronous renderer/window failures use `window.error`
- server must never panic because of malformed client input

---

## 15. Security boundaries

## 15.1 Trust model

Sandboxed agent code is less trusted than the host user session.

The host UI service is trusted but must stay narrow.

## 15.2 What sandboxed code is allowed to do

Through the bridge, sandboxed code may:
- ask the host to open a Glimpse window using supplied HTML/options
- receive page events/messages
- update or close its own windows

## 15.3 What sandboxed code is not allowed to do

Sandboxed code may **not**:
- access host display/session sockets directly
- load arbitrary host file paths through the service
- request arbitrary host process execution
- obtain a generic host RPC channel
- impersonate other client sessions/windows

## 15.4 Webview isolation requirement

In socket-backed sandbox mode, renderer implementations should prefer **ephemeral/isolation-friendly webview state** for AGS-owned windows.

At minimum, the bridge must not intentionally grant access to host browser sessions, cookies, or local host file content.

## 15.5 Input validation requirements

The service must validate:
- method names
- payload sizes
- option enums (`follow_mode`, `cursor_anchor`)
- window ownership per connection
- required types for all fields

## 15.6 No shell interpolation

If the host UI service spawns renderer processes, it must do so with direct exec-style argument passing, not `sh -c`.

---

## 16. Session and ownership rules

### 16.1 Window ownership

A window belongs to the client connection that created it.

Only that connection may:
- update it
- close it
- receive its events

### 16.2 Disconnect behavior

When a client disconnects unexpectedly:
- all windows created by that client must be closed
- child renderer processes owned only by those windows must be cleaned up

### 16.3 AGS process shutdown

When AGS shuts down the host UI service:
- the service should close all remaining windows
- emit best-effort diagnostics to stderr/logs
- exit cleanly

---

## 17. Compatibility story across repos

### 17.1 `agent-sandbox`

Owns:
- service startup and shutdown
- runtime dir/socket mount
- env injection
- operator diagnostics and docs

### 17.2 `../glimpse`

Owns:
- client transport abstraction
- preserving `open()` / `prompt()` semantics
- mapping socket events back into the current EventEmitter-style API

### 17.3 `../glimpse_rust`

Owns:
- protocol implementation
- session/window bookkeeping
- renderer adapter(s)
- readiness and error behavior

### 17.4 Version rule

Until a richer negotiation scheme is needed, all three repos must agree on:
- `AGS_HOST_UI_PROTOCOL=1`
- protocol `v = 1`

Any breaking change requires bumping that protocol version.

---

## 18. Testing requirements implied by this RFC

Minimum coverage expected downstream:

- `../glimpse_rust`
  - protocol parsing/validation
  - window ownership rules
  - disconnect cleanup
  - structured error behavior

- `../glimpse`
  - transport selection in `auto`
  - socket transport request/response/event behavior
  - API compatibility with current `open()` / `prompt()` semantics

- `agent-sandbox`
  - sidecar startup/readiness timeout
  - runtime dir/socket mount wiring
  - env injection
  - shutdown/cleanup behavior

A mock renderer is acceptable in CI; native UI smoke tests can remain host-only.

---

## 19. Why we are not starting with a daemon

A daemon is plausible later, but it is not required for MVP because the current expected workload is mostly **short-lived windows and prompts**.

Starting with a session-scoped service keeps:
- implementation simpler
- failure boundaries clearer
- operator expectations straightforward

We still design the protocol so it can later survive a move to a shared daemon.

---

## 20. Open follow-ups after this RFC

These are implementation follow-ups, not blockers for the spec:

- exact service CLI shape in `../glimpse_rust`
- whether the macOS adapter spawns the existing Glimpse renderer or embeds it differently
- whether socket mode supports `loadFile` later under an allowlisted policy
- whether AGS should emit a dedicated host-UI prompt hint in addition to env vars
- how the older notify-only proxy draft should be revised or superseded for narrow notification use cases

---

## 21. Summary

The approved direction is:

- **host-owned UI service**
- **Unix socket protocol**
- **AGS-managed per-session lifecycle**
- **Glimpse client keeps the same public API**
- **sandbox policy stays above the renderer**

That gives us the correct abstraction now, without forcing a permanent daemon yet.
