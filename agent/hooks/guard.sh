#!/usr/bin/env bash
# guard.sh — Claude Code PreToolUse hook
# Translated from agent/extensions/guard.ts (Pi extension)
#
# Blocks tool calls that:
#   - Access sensitive paths (SSH keys, credentials, etc.)
#   - Read/write outside allowed sandbox roots
#   - Write to secret-like files (.env, .pem, .key, id_*)
#   - Execute dangerous bash commands (rm -rf /, git reset --hard, etc.)
#
# Exit codes:
#   0 — allow the tool call
#   2 — block the tool call (reason printed to stderr)
#
# Non-zero non-2 exits are treated as non-blocking errors by Claude Code
# (fail-open), which is acceptable since the container is the primary
# security boundary.

set -euo pipefail

INPUT=$(cat)

TOOL_NAME=$(printf '%s' "$INPUT" | jq -r '.tool_name // empty')
[[ -z "$TOOL_NAME" ]] && exit 0

# ─── Configuration ────────────────────────────────────────────────────

HOME_DIR="${HOME:-/home/dev}"

# Allowed roots (set by ags plan builder via env, fall back to cwd + /tmp)
_read_json="${AGS_GUARD_READ_ROOTS_JSON:-}"
_write_json="${AGS_GUARD_WRITE_ROOTS_JSON:-}"
[[ -z "$_read_json" ]]  && _read_json="[\"${PWD}\",\"/tmp\"]"
[[ -z "$_write_json" ]] && _write_json="[\"${PWD}\",\"/tmp\"]"

mapfile -t READ_ROOTS  < <(printf '%s' "$_read_json"  | jq -r '.[]')
mapfile -t WRITE_ROOTS < <(printf '%s' "$_write_json" | jq -r '.[]')

SENSITIVE_PATHS=(
  "$HOME_DIR/.ssh"
  "$HOME_DIR/.gnupg"
  "$HOME_DIR/.aws"
  "$HOME_DIR/.config/gcloud"
  "$HOME_DIR/.git-credentials"
  "$HOME_DIR/.npmrc"
  "$HOME_DIR/.pi/agent/auth.json"
  "$HOME_DIR/.pi/agent-host/auth.json"
  "$HOME_DIR/.pi/agent-host/sandbox.json"
)

# ─── Helpers ──────────────────────────────────────────────────────────

deny() { printf '%s\n' "$1" >&2; exit 2; }

# Resolve a path to absolute form, handling ~ and relative paths.
# Uses realpath -m so non-existent paths are still normalised.
resolve_path() {
  local p="$1"
  [[ "$p" == "~" ]]    && p="$HOME_DIR"
  [[ "$p" == "~/"* ]]  && p="$HOME_DIR/${p:2}"
  [[ "$p" != /* ]]     && p="$PWD/$p"
  realpath -m -- "$p" 2>/dev/null || printf '%s' "$p"
}

# True when target is equal to or nested under root.
is_inside() {
  local target="$1" root="$2"
  [[ "$target" == "$root" || "$target" == "$root/"* ]]
}

is_sensitive() {
  local target="$1"
  for p in "${SENSITIVE_PATHS[@]}"; do
    is_inside "$target" "$p" && return 0
  done
  return 1
}

# Check whether target falls inside any of the supplied roots.
# Usage: in_roots "$target" "${ROOTS_ARRAY[@]}"
in_roots() {
  local target="$1"; shift
  for root in "$@"; do
    is_inside "$target" "$(resolve_path "$root")" && return 0
  done
  return 1
}

# Extract the filesystem path from tool_input.
# Read/Write/Edit use "file_path"; Grep/Glob use "path".
get_path() {
  local raw
  raw=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // .tool_input.path // empty')
  if [[ -n "$raw" ]]; then
    resolve_path "$raw"
  fi
}

# ─── Guards ───────────────────────────────────────────────────────────

case "$TOOL_NAME" in

  # ── Read tools ────────────────────────────────────────────────────
  Read|Grep|Glob)
    target=$(get_path)
    if [[ -n "$target" ]]; then
      is_sensitive "$target" \
        && deny "Sensitive path is not readable: $target"
      in_roots "$target" "${READ_ROOTS[@]}" \
        || deny "Read outside sandbox roots denied: $target"
    fi
    ;;

  # ── Write tools ───────────────────────────────────────────────────
  Write|Edit)
    target=$(get_path)
    [[ -z "$target" ]] && deny "$TOOL_NAME requires a file path"

    is_sensitive "$target" \
      && deny "Sensitive path is not writable: $target"
    in_roots "$target" "${WRITE_ROOTS[@]}" \
      || deny "Write outside sandbox roots denied: $target"

    # Deny writes to secret-like files
    if printf '%s' "$target" | grep -qEi '\.(env(\..+)?|pem|key)$'; then
      deny "Refusing writes to secret-like file: $target"
    fi
    if printf '%s' "$target" | grep -qE 'id_[a-z0-9_-]+$'; then
      deny "Refusing writes to secret-like file: $target"
    fi
    ;;

  # ── Bash ──────────────────────────────────────────────────────────
  Bash)
    cmd=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty')

    # Sensitive path references
    for sp in "${SENSITIVE_PATHS[@]}"; do
      [[ "$cmd" == *"$sp"* ]] \
        && deny "Command references sensitive host path"
    done

    # Dangerous patterns (order mirrors guard.ts)
    printf '%s' "$cmd" | grep -qE  '\brm\s+-[a-zA-Z]*r[a-zA-Z]*f\b.*\s\/'   && deny "Refusing recursive force delete on absolute path"
    printf '%s' "$cmd" | grep -qEi '\bgit\s+reset\s+--hard\b'                 && deny "Refusing git reset --hard"
    printf '%s' "$cmd" | grep -qEi '\bgit\s+clean\b[^\n]*\b-f\b'             && deny "Refusing git clean with force flag"
    printf '%s' "$cmd" | grep -qEi '\bmkfs(\.[a-z0-9]+)?\b'                   && deny "Refusing filesystem formatting command"
    printf '%s' "$cmd" | grep -qEi '\bdd\b[^\n]*\bof=\/dev\/'                 && deny "Refusing dd writes to block devices"
    printf '%s' "$cmd" | grep -qEi '\b(shutdown|reboot|poweroff|halt)\b'       && deny "Refusing system power command"
    printf '%s' "$cmd" | grep -qE  ':\(\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;\s*:' && deny "Refusing fork bomb"

    # dcg integration (fail-open: only block on exit 1)
    if command -v dcg &>/dev/null; then
      dcg_exit=0
      dcg_out=$(dcg test --format json "$cmd" 2>/dev/null) || dcg_exit=$?
      if [[ "$dcg_exit" -eq 1 ]]; then
        reason=$(printf '%s' "$dcg_out" \
          | jq -r '.reason // .explanation // .rule_id // empty' 2>/dev/null) \
          || reason=""
        [[ -z "$reason" ]] && reason="Blocked by destructive_command_guard"
        deny "$reason"
      fi
    fi
    ;;

esac

# Allow by default
exit 0
