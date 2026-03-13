#!/usr/bin/env bash
# guard.sh — Claude Code PreToolUse hook
# Translated from agent/extensions/guard.ts (Pi extension)
#
# Blocks tool calls that:
#   - Access sensitive paths (SSH keys, credentials, etc.)
#   - Read/write outside allowed sandbox roots
#   - Write to secret-like files (.env, .pem, .key, id_*)
#   - Reference sensitive host paths from Bash commands
#
# For Bash command classification, this hook intentionally delegates to
# destructive_command_guard (dcg) when available instead of maintaining a
# separate AGS regex denylist.
#
# Exit codes:
#   0 — allow the tool call (or pass through dcg's allow/deny JSON)
#   2 — block the tool call (reason printed to stderr)
#
# If dcg is unavailable or errors, this wrapper fails open and relies on the
# sandbox plus the AGS-specific path protections above.

set -euo pipefail

# Explicit AGS escape hatch for runs that intentionally disable guard layers.
[[ "${AGS_GUARD_YOLO:-0}" == "1" ]] && exit 0

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

    # Sensitive path references remain AGS-specific because they reflect
    # sandbox boundary concerns rather than general destructive intent.
    for sp in "${SENSITIVE_PATHS[@]}"; do
      [[ "$cmd" == *"$sp"* ]] \
        && deny "Command references sensitive host path"
    done

    # Delegate shell command classification to dcg using the original hook
    # payload so Claude-compatible hook output can pass through unchanged.
    # If dcg is missing or errors, fail open.
    if command -v dcg &>/dev/null; then
      dcg_stdout=$(mktemp) || exit 0
      dcg_stderr=$(mktemp) || { rm -f "$dcg_stdout"; exit 0; }

      if printf '%s' "$INPUT" | dcg >"$dcg_stdout" 2>"$dcg_stderr"; then
        [[ -s "$dcg_stderr" ]] && cat "$dcg_stderr" >&2
        [[ -s "$dcg_stdout" ]] && cat "$dcg_stdout"
        rm -f "$dcg_stdout" "$dcg_stderr"
        exit 0
      fi

      rm -f "$dcg_stdout" "$dcg_stderr"
    fi
    ;;

esac

# Allow by default
exit 0
