#!/usr/bin/env python3
"""Read boot environment from an anonymous descriptor; then hand off item FDs."""
import json
import os
import re
import sys

try:
    with os.fdopen(3, "rb") as source:
        environment = json.load(source)
    if not isinstance(environment, dict):
        raise ValueError()
    for name, value in environment.items():
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name) or not isinstance(value, str) or "\0" in value:
            raise ValueError()
    count = int(sys.argv[1])
    command = sys.argv[2:]
    if count < 0 or not command:
        raise ValueError()
except (OSError, ValueError, TypeError, IndexError):
    print("[ags] invalid T3 boot descriptor", file=sys.stderr)
    sys.exit(1)

os.environ.update(environment)
if count:
    # Item descriptors start at 4; the existing item bootstrap expects 3.
    for target in range(3, 3 + count):
        os.dup2(target + 1, target, inheritable=True)
    os.close(3 + count)
    command = ["/run/ags-t3/onepassword-bootstrap", "--fd-count", str(count), "--", *command]
os.execvpe(command[0], command, os.environ)
