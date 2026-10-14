#!/usr/bin/env python3
"""Drive the real AGS checklist through a controlling terminal, with a deadline."""
import errno
import fcntl
import json
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time

keys = bytes.fromhex(sys.argv[1])
pid, master = pty.fork()
if pid == 0:
    fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 180, 0, 0))
    os.environ["TERM"] = "xterm-256color"
    os.execv(sys.argv[2], sys.argv[2:])

output = bytearray()
sent = False
deadline = time.monotonic() + 45
status = None
try:
    while time.monotonic() < deadline:
        readable, _, _ = select.select([master], [], [], 0.05)
        if readable:
            try:
                data = os.read(master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            output.extend(data)
            if not sent and b"Space:" in output and b"Esc/Ctrl-C:" in output:
                if target := os.environ.get("AGS_T3_TEST_STOP_BEFORE_SELECT"):
                    subprocess.run([sys.argv[2], "t3", "stop", target],
                                   stdin=subprocess.DEVNULL, capture_output=True, check=True)
                if target := os.environ.get("AGS_T3_TEST_UPGRADE_BEFORE_SELECT"):
                    subprocess.run([sys.argv[2], "t3", "upgrade", target],
                                   stdin=subprocess.DEVNULL, capture_output=True, check=True)
                os.write(master, keys)
                sent = True
            if not data:
                _, status = os.waitpid(pid, 0)
                break
        child, status = os.waitpid(pid, os.WNOHANG)
        if child:
            break
        status = None
    if status is None:
        child, status = os.waitpid(pid, os.WNOHANG)
        if not child:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
            raise RuntimeError(f"AGS checkbox interaction timed out (sent={sent}): {output.decode('utf8', errors='replace')!r}")
    flags = termios.tcgetattr(master)[3]
    print(json.dumps({"exit": os.waitstatus_to_exitcode(status), "sent": sent,
                      "restored": bool(flags & termios.ECHO and flags & termios.ICANON),
                      "output": output.decode("utf8", errors="replace")}))
finally:
    os.close(master)
