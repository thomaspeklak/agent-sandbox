# T3 v0.0.45 SSH contract

Shell templates transcribed from `packages/ssh/src/tunnel.ts` at upstream tag
`v0.0.45`, blob `0473ffeceae29ce3bc61ba94639d3913e0bd166e`:
https://github.com/pingdotgg/t3code/blob/v0.0.45/packages/ssh/src/tunnel.ts

Upstream is MIT licensed (copyright T3 Code contributors). Comments and blank
lines are ignored by the adapter; executable shell structure is retained.
`@@...@@` placeholders represent upstream-generated content. Node discovery and
development-only JavaScript blocks are matched as opaque data and are never
executed. Numeric placeholders use the release's actual constants.

The bridge recognizes these templates as typed operations. It never executes
the desktop's original shell script, runner, downloader, or stop commands.
Launch returns the actual AGS-owned server's loopback port with external
ownership. Pairing invokes the installed runtime inside the container. Stop
acknowledges transport teardown without stopping or starting an environment.

Forwarding is a separate SSH connection (`-N -L port:127.0.0.1:remotePort`).
`remoteStateKey` is the first 16 hexadecimal characters of the target's SHA-256.
