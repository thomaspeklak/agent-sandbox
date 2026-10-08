#!/usr/bin/env python3
"""Podman protocol fixture. It never evaluates container commands on the host."""
import fcntl
import hashlib
import json
import os
import sys
import time
from pathlib import Path

root = Path(os.environ["AGS_T3_TEST_ROOT"])
state_path = root / "container.json"
args = sys.argv[1:]
with (root / "events.lock").open("a+") as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    with (root / "events.jsonl").open("a") as events:
        events.write(json.dumps(args) + "\n")

def state():
    return json.loads(state_path.read_text()) if state_path.exists() else None

def save(value):
    temporary = state_path.with_suffix(f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value))
    temporary.replace(state_path)

def version(value):
    for mount in value["Mounts"]:
        if mount["Destination"].endswith("/.t3/runtime"):
            return (Path(mount["Source"]) / "version").read_text().strip()
    raise RuntimeError("missing managed runtime mount")

if args == ["--version"]:
    print("podman version 5.6.0")
elif args[:1] == ["version"]:
    print("5.6.0")
elif args[:3] == ["system", "connection", "list"]:
    pass
elif args[:2] == ["image", "exists"]:
    pass
elif args[:2] == ["image", "inspect"]:
    print("sha256:" + "a" * 64)
elif args[:2] == ["container", "exists"]:
    sys.exit(0 if state() else 1)
elif args[:2] == ["container", "inspect"]:
    print(json.dumps([state()]))
elif args[0] == "create":
    labels = {}
    mounts = []
    environment = []
    for index, arg in enumerate(args):
        if arg.startswith("--label="):
            key, value = arg[len("--label="):].split("=", 1)
            labels[key] = value
        if arg == "-v":
            source, destination, mode = args[index + 1].rsplit(":", 2)
            mounts.append({"Source": source, "Destination": destination, "RW": mode == "rw"})
        if arg == "-e":
            environment.append(args[index + 1])
    save({"Id": "c" * 64, "Image": "sha256:" + "a" * 64, "Config": {"Labels": labels, "Env": environment},
          "Mounts": mounts, "State": {"Running": False}, "ready": False})
    print("fixture-container")
elif args[0] == "start":
    value = state()
    value["State"]["Running"] = True
    value["ready"] = False
    save(value)
elif args[0] == "stop":
    value = state()
    value["State"]["Running"] = False
    save(value)
    if value.get("server_pid"):
        try:
            os.kill(value["server_pid"], 15)
        except ProcessLookupError:
            pass
elif args[0] == "rm":
    state_path.unlink()
elif args[0] == "exec":
    if any(arg.startswith("--preserve-fds=") for arg in args):
        with os.fdopen(3, "rb") as source:
            environment = json.load(source)
        value = state()
        value["ready"] = True
        value["server_pid"] = os.getpid()
        value["secret_hash"] = hashlib.sha256(environment.get("T3_TEST_SECRET", "").encode()).hexdigest()
        save(value)
        while state() and state()["State"]["Running"]:
            time.sleep(0.025)
    elif "--version" in args:
        print(version(state()))
    elif "wait-ready" in args:
        deadline = time.monotonic() + 5
        while not state()["ready"]:
            if time.monotonic() > deadline:
                sys.exit(1)
            time.sleep(0.025)
    elif "pairing" in args:
        print('{"credential":"fixture-pairing"}')
    elif "/run/ags-t3/stop-server" in args:
        value = state()
        if value.get("server_pid"):
            try:
                os.kill(value["server_pid"], 15)
            except ProcessLookupError:
                pass
    elif "socat" in args:
        data = sys.stdin.buffer.read()
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
    else:
        raise RuntimeError(f"unexpected exec operation {args}")
else:
    raise RuntimeError(f"unexpected Podman operation {args}")
