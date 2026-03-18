# Sandbox App Origin Relay for Host-Owned Glimpse Webviews

Status: Accepted MVP extension to `docs/GLIMPSE_HOST_UI_BRIDGE.md`

Related issues:
- `agent-sandbox-mp3.9`
- `agent-sandbox-mp3.10`
- `agent-sandbox-mp3.11`
- `agent-sandbox-mp3.12`
- `agent-sandbox-1by`

---

## Problem

Some consumers spin up a temporary HTTP app server inside the AGS sandbox and expect the webview to behave like a normal browser pointed at that app origin.

The original relay multiplexed all served-app traffic through one host port using `/t/<token>/...` paths. That breaks ordinary apps which assume they live at origin root, because root-absolute requests like these escape the relay prefix:

- `/styles.css`
- `/script.js`
- `/submit`
- `/media?path=...&session=...`

A host-owned Glimpse window also cannot use `http://127.0.0.1:<port>` directly, because that `localhost` is the **host**, not the **sandbox**.

---

## Chosen direction

AGS provides a **session-scoped dedicated-origin relay**.

### Pieces

1. **Host registration socket**
   - listens on a Unix socket in the mounted relay runtime dir
   - accepts `register` requests from sandbox helpers
   - allocates one random host loopback port per registration

2. **Host HTTP listener per app**
   - binds `127.0.0.1:<random-port>` on the host
   - forwards all requests for that port to exactly one sandbox-local app

3. **Sandbox relay shim**
   - runs inside the container
   - listens on a second Unix socket in the same mounted runtime dir
   - receives host-forwarded HTTP requests and relays them to `127.0.0.1:<sandbox-port>` inside the container

4. **Sandbox helper (`ags-webview-url`)**
   - called by sandbox-side code that needs an explicit relay URL
   - registers a local app port with the host registration socket
   - prints the final host-reachable URL

In the standard Glimpse-served-app flow, package code should not call this helper directly. `glimpseui` owns localhost-to-host relay resolution when running under AGS.

---

## Why this design

This keeps the trust boundary narrow while restoring normal origin semantics:

- each served app gets its own host origin
- root-absolute URLs work again without path-prefix hacks
- AGS still mounts only a small runtime dir/socket pair into the sandbox
- AGS still owns lifecycle and cleanup
- there is still no generic host-to-container TCP bridge

The host port itself is the routing key. Tokens are no longer needed for the primary served-app flow.

---

## Contract

## Runtime paths and env

Host runtime dir:
- `${XDG_RUNTIME_DIR:-/tmp}/ags-webview-relay-<pid>`

Mounted into container at:
- `/run/ags-webview-relay`

Container env:
- `AGS_WEBVIEW_RELAY_SOCKET=/run/ags-webview-relay/relay.sock`
- `AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET=/run/ags-webview-relay/upstream.sock`

Helper path inside container:
- `~/.local/bin/ags-webview-url`

Notes:
- `AGS_WEBVIEW_RELAY_SOCKET` is for sandbox helpers registering apps
- `AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET` is for the sandbox relay shim receiving forwarded requests from the host

## Helper contract

Usage:

```bash
ags-webview-url <port> [base_path]
```

Examples:

```bash
ags-webview-url 4173
ags-webview-url 3000 /app
```

Output:

```text
http://127.0.0.1:43125/
http://127.0.0.1:43126/app
```

Rules:
- `port` is the sandbox-local TCP port where the app server is listening
- `base_path` defaults to `/`
- the returned URL is the URL the **host-owned webview must load**
- AGS allocates a new random host port for each registration

## Registration protocol

One JSON request per connection, one JSON response.

Request:

```json
{"type":"register","port":4173,"base_path":"/"}
```

Response:

```json
{
  "ok": true,
  "host_port": 43125,
  "base_path": "/",
  "url": "http://127.0.0.1:43125/"
}
```

## Host → shim forwarding protocol

The host listener forwards requests over the upstream Unix socket.

Request:

```json
{
  "type": "http_request",
  "port": 4173,
  "base_path": "/app",
  "method": "GET",
  "path": "/styles.css?theme=dark",
  "headers": [["accept", "text/css"]],
  "body_base64": null
}
```

Response:

```json
{
  "ok": true,
  "status": 200,
  "reason": "OK",
  "headers": [["content-type", "text/css; charset=utf-8"]],
  "body_base64": "Ym9keSB7fQ=="
}
```

### Base path behavior

`base_path` remains supported, but routing is now port-based.

Rules:
- if `base_path` is `/`, requests are forwarded unchanged
- if `base_path` is non-root, the shim prepends it when the browser requests a root-absolute path like `/styles.css`
- if the browser already requests a path under that base path, such as `/app` or `/app/media?...`, the shim does **not** duplicate the prefix

This keeps both of these working:

- initial URL: `http://127.0.0.1:<host-port>/app`
- later root-absolute requests: `/styles.css`, `/media?...`

---

## Glimpse contract change

The original host UI bridge RFC focused on inline HTML. This extension keeps the same split:

- `html` source
- `url` source

For direct Glimpse usage, that maps to:
- `open(html, options)` for inline HTML
- `open(url, options)` or `openURL(url, options)` for served-app flows

Ownership boundary:
- packages/tools construct their ordinary localhost URL (for example `http://localhost:4173/?session=abc`)
- `glimpseui` detects sandbox-local localhost URLs under AGS and resolves them through this relay automatically
- AGS owns dedicated host-port allocation and HTTP forwarding
- browser-open fallbacks that pass through the AGS auth proxy may also offer a **Proxy** choice for the same localhost-with-port URLs; that uses this relay too, but Glimpse remains the primary owner of served-app URL resolution for packages that already use Glimpse directly

For the host UI service protocol, `open`/`prompt` accept a source union rather than HTML-only payloads.

---

## Security boundaries

The relay still must not expose arbitrary container networking.

Allowed:
- registering a sandbox app and receiving one dedicated host origin
- forwarding requests from that host port only to the registered sandbox-local destination

Not allowed:
- raw host-to-container TCP forwarding
- wildcard access to arbitrary sandbox ports
- using the relay as a general-purpose HTTP proxy

Session isolation story:
- relay runtime dirs are session-scoped
- listeners are allocated only by the AGS session that owns that runtime dir
- listeners bind only on host loopback
- AGS container networking still does not expose host loopback by default (`localhost` remains container-local)
- listeners are removed when the AGS session exits

There is no `/t/<token>/...` compatibility path in the primary flow anymore.

---

## Current MVP implementation status

Implemented now:
- host registration socket
- one host localhost listener per registered sandbox app
- sandbox helper returning the final host URL
- sandbox relay shim that forwards by `(sandbox_port, base_path)`
- compatibility for root-absolute asset/API/media URLs
- `openURL()` / `loadURL()` support in Glimpse direct mode

Not implemented yet:
- WebSocket relay
- SSE/streaming-aware proxying
- long-lived shared daemon mode
- host UI service integration for served-app mode beyond the current contract

For now, if an app requires WebSockets or streaming semantics, it remains follow-up work.

---

## Practical guidance

Use **inline HTML** when:
- the page is small
- you do not need a real app origin
- all state can flow through `window.glimpse.send()` and `win.send()`

Use **served-app mode** when:
- you need multiple assets/modules
- you need normal browser fetches against an origin
- you already have a temporary local app server pattern
- the app uses root-absolute asset or API URLs

---

## Summary

For sandbox-local app servers, the model is now:

- sandbox app listens on local port
- sandbox code calls `ags-webview-url <port>`
- AGS allocates one dedicated host loopback port for that app
- the helper returns a host URL rooted at that origin
- the host-owned Glimpse webview loads that URL

This restores ordinary origin behavior without broadening the sandbox trust boundary.
