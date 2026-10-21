# Reduced v0.0.45 desktop pairing request, independent of AGS's renderer.
set -eu
STATE_DIR="$HOME/.t3/ssh-launch/0123456789abcdef"
DEFAULT_SERVER_HOME="$HOME/.t3"
RUNNER_FILE="$STATE_DIR/run-t3.sh"
cat >"$RUNNER_FILE" <<'SH'
#!/bin/sh
set -eu
T3_NODE_SCRIPT_PATH=""
T3_ARCHIVE_VERSION="0.0.45"
T3_RUNTIME_DIR="$HOME/.t3/runtime/versions/$T3_ARCHIVE_VERSION"
touch '@@EXECUTION_MARKER@@'
exit 99
exec "$T3_RUNTIME_DIR/t3" "$@"
SH
PAIRING_BASE_DIR="$DEFAULT_SERVER_HOME"
"$RUNNER_FILE" auth pairing create --base-dir "$PAIRING_BASE_DIR" --json
