#!/usr/bin/env python3
"""Signal the environment's T3 server before stopping its container."""
import json
import os
import signal
import sys
import time

try:
    with open(sys.argv[1], encoding="utf8") as source:
        runtime = json.load(source)
    pid = runtime["pid"]
    if not isinstance(pid, int) or pid <= 1:
        raise ValueError("invalid server PID")
    os.kill(pid, signal.SIGTERM)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            break
        time.sleep(0.05)
except (FileNotFoundError, ProcessLookupError):
    pass
