# Reduced v0.0.45 desktop request, independent of AGS's template renderer.
# Quote style, helper serialization, and installer implementation may vary.
set -eu
STATE_KEY="$1"
STATE_DIR="$HOME/.t3/ssh-launch/$STATE_KEY"
DEFAULT_SERVER_HOME="$HOME/.t3"
RUNNER_NEXT="$STATE_DIR/run-t3.next.$$"
cat >"$RUNNER_NEXT" <<'SH'
#!/bin/sh
set -eu
T3_NODE_SCRIPT_PATH=""
T3_ARCHIVE_VERSION="0.0.45"
T3_RUNTIME_DIR="$HOME/.t3/runtime/versions/$T3_ARCHIVE_VERSION"
ensure_remote_node_path() { node -e 'process.exit(99)'; }
t3_fetch() { exit 99; }
# The adapter must never run this installer, even inside the container.
touch '@@EXECUTION_MARKER@@'
exit 99
exec "$T3_RUNTIME_DIR/t3" "$@"
SH
T3_ARCHIVE_MODE=1
pick_port() { "$RUNNER_NEXT" __ssh-helper pick-port; }
REMOTE_PORT="$(pick_port)"
REMOTE_MANAGED=external
printf '{"remotePort":%s,"serverKind":"%s"}\n' "$REMOTE_PORT" "${REMOTE_MANAGED:-managed}"
