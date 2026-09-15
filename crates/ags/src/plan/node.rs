//! Entrypoint setup for AGS-managed Node runtime selection.

/// Commands run before the selected agent or shell. The wrappers resolve the
/// current directory on every invocation, so `cd` and noninteractive shells
/// do not depend on `mise activate` prompt hooks.
pub(super) const NODE_WRAPPER_SETUP: &str = r#"mkdir -p /home/dev/.local/bin
cat > /home/dev/.local/bin/ags-node-runtime <<'AGS_NODE_RUNTIME'
#!/usr/bin/env bash
set -u

name="${0##*/}"

# Node-based agent launchers use the fixed image Node exactly once. The wrapper
# removes the marker before starting Node, so commands the agent subsequently
# launches still use the project-selected runtime.
if [ "${AGS_NODE_AGENT_BOOTSTRAP:-}" = "1" ] && [ "$name" = "node" ]; then
  exec env -u AGS_NODE_AGENT_BOOTSTRAP /usr/bin/node "$@"
fi

workspace="${AGS_NODE_WORKSPACE_ROOT:-}"
dir="${PWD:-}"
version_file=""

# Only search inside the AGS workdir mount. This intentionally does not use
# mise's project discovery, so project mise.toml files, hooks, and env entries
# are never evaluated.
if [ -n "$workspace" ] && [ -n "$dir" ]; then
  while :; do
    case "$dir" in
      "$workspace"|"$workspace"/*)
        if [ -L "$dir/.nvmrc" ]; then
          echo "[ags] .nvmrc is a symlink and is not allowed: $dir/.nvmrc. The file was not executed." >&2
          exit 2
        fi
        if [ -f "$dir/.nvmrc" ]; then
          version_file="$dir/.nvmrc"
          break
        fi
        ;;
      *) break ;;
    esac
    [ "$dir" = "$workspace" ] && break
    parent="${dir%/*}"
    [ "$parent" = "$dir" ] && break
    [ -n "$parent" ] || parent=/
    dir="$parent"
  done
fi

if [ -n "$version_file" ]; then
  if [ "$(wc -l < "$version_file")" -gt 1 ]; then
    echo "[ags] invalid .nvmrc at $version_file; expected one numeric Node version line. The file was not executed." >&2
    exit 2
  fi
  version="$(cat -- "$version_file")"
  case "$version" in
    *$'\r') version="${version%$'\r'}";;
  esac
  case "$version" in
    *[!vV0-9.]*|""|.*|*.|*..*|*.*.*.*)
      echo "[ags] invalid .nvmrc at $version_file; expected a numeric Node version such as 22 or 22.14.0. The file was not executed." >&2
      exit 2
      ;;
  esac
  case "$version" in
    v*|V*) version="${version#?}" ;;
  esac
  case "$version" in
    ''|*[!0-9.]*|.*|*.|*..*)
      echo "[ags] invalid .nvmrc at $version_file; expected a numeric Node version such as 22 or 22.14.0. The file was not executed." >&2
      exit 2
      ;;
  esac
  node_root="$(MISE_DATA_DIR="${MISE_DATA_DIR:-/opt/ags/mise}" MISE_CACHE_DIR=/tmp/ags-mise-cache mise --no-config --offline where "node@$version" 2>/dev/null)" || {
    echo "[ags] Node $version requested by $version_file is not installed in the AGS mise store." >&2
    if [ -n "${AGS_NODE_CONFIG_PATH:-}" ]; then
      printf '[ags] Run `ags node install %q --config %q`, then retry.\n' "$version" "$AGS_NODE_CONFIG_PATH" >&2
    else
      printf '[ags] Run `ags node install %q`, then retry.\n' "$version" >&2
    fi
    exit 127
  }
  candidate="$node_root/bin/$name"
  if [ ! -x "$candidate" ]; then
    echo "[ags] Node $version is installed but does not provide $name." >&2
    exit 127
  fi
  exec "$candidate" "$@"
fi

case "$name" in
  node|npm|npx|corepack) exec "/usr/bin/$name" "$@" ;;
  *) echo "[ags] unsupported Node runtime command: $name" >&2; exit 127 ;;
esac
AGS_NODE_RUNTIME
chmod 0755 /home/dev/.local/bin/ags-node-runtime
for name in node npm npx corepack; do ln -sfn ags-node-runtime "/home/dev/.local/bin/$name"; done"#;

#[cfg(test)]
#[path = "node_tests.rs"]
mod tests;
