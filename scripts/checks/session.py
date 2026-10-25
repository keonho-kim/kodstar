# Purpose: Verify shutdown and interruption with an unread result pipe and isolated state.
# Pattern: Integration Check.
# Usage: python3 -B scripts/checks/session.py /absolute/path/to/kodstar.
# Related: crates/kodstar-cli/src/session/mod.rs, crates/kodstar-cli/src/session/transport.rs.

import array
import fcntl
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import tempfile
import termios
import time


def send(stream, frame):
    stream.write(json.dumps(frame).encode() + b"\n")
    stream.flush()


def blocked_session(binary, root, environment, mode):
    process = subprocess.Popen([str(binary), "serve", "--stdio"], cwd=root,
                               env=environment, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        send(process.stdin, {"protocol_version": 2, "id": "query", "method": "query",
                             "params": {"query": {"kind": "symbols", "file": "main.rs"},
                                        "options": {"all": True, "refresh": "none"}}})
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            deadline = time.monotonic() + 15
            previous, stable = -1, time.monotonic()
            while True:
                pending = array.array("i", [0])
                fcntl.ioctl(process.stdout, termios.FIONREAD, pending, True)
                if pending[0] != previous:
                    previous, stable = pending[0], time.monotonic()
                if pending[0] >= 4096 and time.monotonic() - stable >= 0.25:
                    break
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise AssertionError("result pipe did not reach backpressure")
                selector.select(0.01)
        if mode == "cancel":
            send(process.stdin, {"protocol_version": 2, "id": "cancel", "method": "cancel",
                                 "params": {"request_id": "query"}})
            status = subprocess.run([str(binary), "status", "--no-refresh", "--format", "json"],
                                    cwd=root, env=environment, capture_output=True, timeout=3)
            assert status.returncode == 0, status.stderr.decode()
            assert json.loads(status.stdout)["complete"]
            assert process.poll() is None, "request cancellation killed the session"
        if mode in ("shutdown", "cancel"):
            send(process.stdin, {"protocol_version": 2, "id": "stop", "method": "shutdown"})
        else:
            process.send_signal(signal.SIGINT)
        try:
            code = process.wait(timeout=3)
        except subprocess.TimeoutExpired as error:
            raise AssertionError(f"{mode} was blocked by unread stdout") from error
        assert code in (0, 130), (code, process.stderr.read().decode())
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        for stream in (process.stdin, process.stdout, process.stderr):
            stream.close()


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="kodstar-session-check-") as temporary:
        base = Path(temporary)
        root, home, scratch = (base / name for name in ("project", "home", "scratch"))
        for directory in (root, home, scratch):
            directory.mkdir()
        environment = {**os.environ, "HOME": str(home), "TMPDIR": str(scratch),
                       "ZDOTDIR": str(home)}
        (root / "main.rs").write_text("".join(f"fn symbol_{n}() {{}}\n" for n in range(6000)))
        subprocess.run([str(binary), "init", "--yes", "--jobs", "1"], cwd=root,
                       env=environment, check=True, capture_output=True, timeout=90)
        for mode in ("shutdown", "cancel", "interrupt"):
            blocked_session(binary, root, environment, mode)
            assert not list(scratch.iterdir()), "session left temporary frame files"
    assert not Path(temporary).exists()
    print("Session: unread-output shutdown, request cancellation, interruption and cleanup passed.")


if __name__ == "__main__":
    main()
