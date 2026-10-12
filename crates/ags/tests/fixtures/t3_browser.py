#!/usr/bin/env python3
"""Browser protocol fixture: real loopback listener and distinct assigned ports."""
import json
import os
import pathlib
import socket
import sys

root = pathlib.Path(os.environ["AGS_T3_TEST_ROOT"])
args = dict(arg.split("=", 1) for arg in sys.argv[1:] if "=" in arg)
assert args["--remote-debugging-port"] == "0"
profile = pathlib.Path(args["--user-data-dir"])
history = root / "browser-ports.json"
ports = json.loads(history.read_text()) if history.exists() else []
while True:
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
    if port not in ports:
        break
    listener.close()
listener.listen()
history.write_text(json.dumps(ports + [port]))
(profile / "DevToolsActivePort").write_text(f"{port}\n")
while True:
    connection, _ = listener.accept()
    connection.close()
