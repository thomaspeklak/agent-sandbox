# Executed in the updater's fresh --network=none container with runtime mounts ro.
T3_VERIFY_DIR="$(mktemp -d /tmp/ags-t3-verify-XXXXXX)"
T3_VERIFY_VERSION="$(cat /usr/local/pnpm/ags-t3-runtime/version)"
T3_VERIFY_BIN="/usr/local/pnpm/ags-t3-runtime/versions/$T3_VERIFY_VERSION/t3"
mkdir -p "$T3_VERIFY_DIR/home/.t3/userdata"
printf '%s\n' '{"enableProviderUpdateChecks":false,"providers":{"codex":{"enabled":false},"claudeAgent":{"enabled":false},"cursor":{"enabled":false},"grok":{"enabled":false},"opencode":{"enabled":false},"antigravity":{"enabled":false}}}' > "$T3_VERIFY_DIR/home/.t3/userdata/settings.json"
env HOME="$T3_VERIFY_DIR/home" "$T3_VERIFY_BIN" serve --host 127.0.0.1 --port 3773 --base-dir "$T3_VERIFY_DIR/home/.t3" --no-browser >"$T3_VERIFY_DIR/server.log" 2>&1 &
T3_VERIFY_PID="$!"
trap 'kill "$T3_VERIFY_PID" 2>/dev/null || true; wait "$T3_VERIFY_PID" 2>/dev/null || true; rm -rf "$T3_VERIFY_DIR"' EXIT
if ! "$T3_VERIFY_BIN" __ssh-helper wait-ready 3773 60000 1000; then
  printf '[ags] T3 offline/read-only server verification failed; diagnostics:\n' >&2
  cat "$T3_VERIFY_DIR/server.log" >&2
  exit 1
fi
kill -0 "$T3_VERIFY_PID"
# AGS_T3_TERMINAL_PROBE
kill "$T3_VERIFY_PID"
wait "$T3_VERIFY_PID" || true
rm -rf "$T3_VERIFY_DIR"
trap - EXIT
