# Purpose: Verify actual terminal acceptance and refusal without modifying the user's HOME.
# Pattern: Integration Check.
# Usage: python3 scripts/checks/confirmation.py /absolute/path/to/kodstar.
# Related: crates/kodstar-cli/src/confirmation.rs, docs/development.md.

import errno
import json
import os
from pathlib import Path
import pty
import selectors
import subprocess
import sys
import tempfile


def confirm(binary, root, home, answer):
    master, slave = pty.openpty()
    process = subprocess.Popen(
        [str(binary), "init", "--format", "json"], cwd=root,
        env={**os.environ, "HOME": str(home), "ZDOTDIR": str(home)},
        stdin=slave, stderr=slave, stdout=subprocess.PIPE,
    )
    os.close(slave)
    try:
        transcript = b""
        with selectors.DefaultSelector() as selector:
            selector.register(master, selectors.EVENT_READ)
            while b"Continue? [y/N]" not in transcript:
                if not selector.select(15):
                    raise AssertionError("terminal confirmation did not arrive")
                try:
                    chunk = os.read(master, 4096)
                except OSError as error:
                    if error.errno == errno.EIO:
                        raise AssertionError("confirmation exited prematurely") from error
                    raise
                if not chunk or len(transcript) > 65536:
                    raise AssertionError("invalid confirmation transcript")
                transcript += chunk
        assert str(root.resolve()).encode() in transcript
        os.write(master, answer.encode() + b"\n")
        output, _ = process.communicate(timeout=30)
        return process.returncode, output
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="kodstar-confirm-") as temporary:
        base = Path(temporary)
        home = base / "user's home"
        root = base / "source"
        home.mkdir()
        root.mkdir()
        (root / "main.rs").write_text("fn confirmed() {}", encoding="utf-8")
        status, output = confirm(binary, root, home, "n")
        assert status == 2 and not output
        assert not (root / ".kodstar").exists()
        assert not (home / ".kodstar").exists()
        status, output = confirm(binary, root, home, "yes")
        assert status == 0
        assert json.loads(output)["history"]["checkpoint"] == "c1"
        env = {**os.environ, "HOME": str(home), "ZDOTDIR": str(home)}
        subprocess.run([str(binary), "uninstall", "--yes"], env=env, check=True,
                       stdout=subprocess.PIPE)
        assert not (home / ".kodstar").exists()
        assert not (root / ".kodstar").exists()
        assert (root / "main.rs").read_text(encoding="utf-8") == "fn confirmed() {}"
    print("Terminal confirmation: refusal, acceptance, target and cleanup passed.")


if __name__ == "__main__":
    main()
