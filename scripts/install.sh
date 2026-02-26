#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
CONFIG_LINK="$HOME/.config/pi-sandbox"
AGENT_DIR="${PI_SBOX_AGENT_DIR:-$HOME/.pi/agent-sandbox}"
HOST_PI_SETTINGS="${PI_SBOX_HOST_PI_SETTINGS:-$HOME/.pi/agent/settings.json}"
SETTINGS_TEMPLATE="$ROOT_DIR/agent/settings.example.json"
STAMP="$(date +%Y%m%d%H%M%S)"

backup_if_needed() {
  local target="$1"
  if [[ -e "$target" || -L "$target" ]]; then
    local backup="${target}.bak.${STAMP}"
    mv "$target" "$backup"
    echo "Backed up: $target -> $backup"
  fi
}

link_path() {
  local source="$1"
  local target="$2"

  mkdir -p "$(dirname "$target")"

  if [[ -L "$target" ]]; then
    local current
    current="$(readlink -f "$target" 2>/dev/null || true)"
    local expected
    expected="$(readlink -f "$source")"
    if [[ "$current" == "$expected" ]]; then
      echo "Already linked: $target"
      return
    fi
    rm -f "$target"
  elif [[ -e "$target" ]]; then
    backup_if_needed "$target"
  fi

  ln -s "$source" "$target"
  echo "Linked: $target -> $source"
}

bootstrap_sandbox_settings() {
  local target="$AGENT_DIR/settings.json"

  if [[ -L "$target" ]]; then
    local current
    current="$(readlink -f "$target" 2>/dev/null || true)"
    if [[ "$current" == "$ROOT_DIR/agent/settings.json" || "$current" == "$ROOT_DIR/agent/settings.example.json" ]]; then
      rm -f "$target"
      echo "Removed legacy settings symlink: $target"
    fi
  fi

  if [[ -f "$target" ]]; then
    echo "Using existing sandbox settings: $target"
    return
  fi

  mkdir -p "$(dirname "$target")"

  if [[ -f "$HOST_PI_SETTINGS" ]]; then
    cp "$HOST_PI_SETTINGS" "$target"
    chmod 600 "$target"
    echo "Copied sandbox settings from host: $HOST_PI_SETTINGS -> $target"
    return
  fi

  if [[ -f "$SETTINGS_TEMPLATE" ]]; then
    cp "$SETTINGS_TEMPLATE" "$target"
    chmod 600 "$target"
    echo "Copied sandbox settings from template: $SETTINGS_TEMPLATE -> $target"
    return
  fi

  echo "Missing sandbox settings source. Expected one of:" >&2
  echo "  $HOST_PI_SETTINGS" >&2
  echo "  $SETTINGS_TEMPLATE" >&2
  exit 1
}

mkdir -p "$BIN_DIR" "$HOME/.config" "$AGENT_DIR/extensions"

# Migrate existing config dir content into project config before linking the directory
if [[ -d "$CONFIG_LINK" && ! -L "$CONFIG_LINK" ]]; then
  echo "Migrating existing $CONFIG_LINK into project config"
  rsync -a "$CONFIG_LINK/" "$ROOT_DIR/config/"
fi

link_path "$ROOT_DIR/config" "$CONFIG_LINK"

# Keep runtime session/auth data in ~/.pi/agent-sandbox; only link managed extension file
bootstrap_sandbox_settings
link_path "$ROOT_DIR/agent/extensions/guard.ts" "$AGENT_DIR/extensions/guard.ts"

chmod +x "$ROOT_DIR/bin/pis-run"

for cmd in pis pisb pis-setup pis-doctor pis-update; do
  chmod +x "$ROOT_DIR/bin/$cmd"
  link_path "$ROOT_DIR/bin/$cmd" "$BIN_DIR/$cmd"
done

chmod +x "$ROOT_DIR/scripts/resolve-config.py" "$ROOT_DIR/scripts/install.sh" "$ROOT_DIR/scripts/uninstall.sh"

# pis-run is internal and should not be linked into PATH
rm -f "$BIN_DIR/pis-run"

# Remove legacy aliases
for legacy in pi-sbox pi-sbox-browser pi-sbox-setup; do
  if [[ -e "$BIN_DIR/$legacy" || -L "$BIN_DIR/$legacy" ]]; then
    rm -f "$BIN_DIR/$legacy"
    echo "Removed legacy alias: $BIN_DIR/$legacy"
  fi
done

echo
echo "Install complete."
echo "Run: pis doctor"
