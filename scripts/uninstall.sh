#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
CONFIG_LINK="$HOME/.config/pi-sandbox"
AGENT_DIR="${PI_SBOX_AGENT_DIR:-$HOME/.pi/agent-sandbox}"

unlink_if_points_to_project() {
  local path="$1"
  if [[ ! -L "$path" ]]; then
    return
  fi

  local current
  current="$(readlink -f "$path" 2>/dev/null || true)"
  if [[ "$current" == "$ROOT_DIR"* ]]; then
    rm -f "$path"
    echo "Removed: $path"
  fi
}

for cmd in pis pisb pis-setup pis-doctor pis-update pis-run; do
  unlink_if_points_to_project "$BIN_DIR/$cmd"
done

unlink_if_points_to_project "$CONFIG_LINK"
unlink_if_points_to_project "$AGENT_DIR/settings.json"
unlink_if_points_to_project "$AGENT_DIR/extensions/guard.ts"

echo "Uninstall complete."
