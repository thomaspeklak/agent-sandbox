---
name: guard
description: Show active sandbox guard status
---

Report the following sandbox guard status to the user. Do not editorialize or suggest changes — just display the information clearly.

## Sandbox status

- AGS_SANDBOX: !`echo "${AGS_SANDBOX:-<not set>}"`
- Detection: !`if [ "${AGS_SANDBOX}" = "1" ]; then echo "ON (AGS_SANDBOX=1)"; elif [ -n "${AGS_GUARD_READ_ROOTS_JSON}" ] || [ -n "${AGS_GUARD_WRITE_ROOTS_JSON}" ]; then echo "ON (guard roots present)"; else echo "OFF (no sandbox markers)"; fi`

## Guard config (informational)

- Read roots env: !`echo "${AGS_GUARD_READ_ROOTS_JSON:-<not set>}"`
- Write roots env: !`echo "${AGS_GUARD_WRITE_ROOTS_JSON:-<not set>}"`
- Policy summary: sensitive-path denies, secret-like write denies, and dcg for Bash

## Context

- Working directory: !`pwd`
- HOME: !`echo "$HOME"`
- Guard hook: !`if [ -x "/home/dev/.config/ags/hooks/guard.sh" ]; then echo "installed"; else echo "NOT FOUND"; fi`
