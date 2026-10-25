# Purpose: Check installer staging cleanup and explicit binary selection in isolated homes.
# Pattern: Integration Check.
# Usage: python3 scripts/checks/installation.py /absolute/path/to/kodstar.
# Related: install.sh, crates/kodstar-cli/src/installation/install.rs.

import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile


STUB = """#!/bin/sh
# Purpose: Supply controlled Cargo build outcomes for installer lifecycle checks.
# Pattern: Test Fixture.
# Usage: The isolated installer invokes this rustup replacement.
# Related: scripts/checks/installation.py.
set -eu
if [ "$3" = rustc ]; then exit 0; fi
while [ "$#" -gt 0 ]; do
  if [ "$1" = --target-dir ]; then shift; target=$1; break; fi
  shift
done
printf '%s' "$target" > "$KODSTAR_CHECK_RECORD"
mkdir -p "$target/dist"
case "$KODSTAR_CHECK_MODE" in
  fail) exit 23 ;;
  interrupt) printf 'BUILD_READY\\n'; cat >/dev/null ;;
  success) cp "$KODSTAR_CHECK_BINARY" "$target/dist/kodstar" ;;
  install-fail)
    printf '#!/bin/sh\\n# Purpose: Fail staged installation.\\n# Pattern: Test Fixture.\\n# Usage: Installer cleanup check.\\n# Related: scripts/checks/installation.py.\\nexit 29\\n' > "$target/dist/kodstar"
    chmod +x "$target/dist/kodstar"
    ;;
esac
"""


def execute(binary, mode):
    with tempfile.TemporaryDirectory(prefix="kodstar-install-check-") as temporary:
        base = Path(temporary).resolve()
        checkout, home, tools, stages = [base / part for part in (
            "source checkout", "user's home", "tools", "temporary builds")]
        for path in (checkout, home, tools, stages):
            path.mkdir()
        root = Path(__file__).resolve().parents[2]
        shutil.copy2(root / "install.sh", checkout / "install.sh")
        shutil.copy2(root / "rust-toolchain.toml", checkout / "rust-toolchain.toml")
        rustup = tools / "rustup"
        rustup.write_text(STUB)
        rustup.chmod(0o755)
        record = base / "build-location"
        env = {**os.environ, "HOME": str(home), "ZDOTDIR": str(home),
               "SHELL": "/bin/zsh", "PATH": str(tools) + ":/usr/bin:/bin",
               "TMPDIR": str(stages), "KODSTAR_CHECK_BINARY": str(binary),
               "KODSTAR_CHECK_MODE": mode, "KODSTAR_CHECK_RECORD": str(record)}
        command = ["/bin/sh", str(checkout / "install.sh")]
        process = subprocess.Popen(command, cwd=checkout, env=env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=True)
        try:
            if mode == "interrupt":
                assert process.stdout.readline() == b"BUILD_READY\n"
                os.killpg(process.pid, signal.SIGTERM)
            stdout, stderr = process.communicate(timeout=30)
            expected = {"success": 0, "fail": 23, "install-fail": 29, "interrupt": 143}[mode]
            assert process.returncode == expected, (mode, process.returncode, stderr)
            target = Path(record.read_text())
            assert stages in target.parents, target
            assert not target.exists(), (mode, target)
            assert not list(stages.iterdir()), mode
            assert not (checkout / "target").exists()
            assert not (checkout / ".kodstar").exists()
            if mode == "success":
                assert (home / ".local/bin/kodstar").is_file()
                assert sorted(path.name for path in (home / ".kodstar").iterdir()) == [
                    "install.json", "installation.lock"]
                subprocess.run([str(home / ".local/bin/kodstar"), "uninstall", "--yes"],
                               env=env, check=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, timeout=15)
                assert not list(home.iterdir())
            else:
                assert not list(home.iterdir()), (mode, stdout, stderr)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate(timeout=10)


def local_binary(binary):
    with tempfile.TemporaryDirectory(prefix="kodstar-binary-check-") as temporary:
        base = Path(temporary).resolve()
        home = base / "home"
        home.mkdir()
        selected = base / "local kodstar"
        shutil.copy2(binary, selected)
        script = Path(__file__).resolve().parents[2] / "install.sh"
        env = {**os.environ, "HOME": str(home), "ZDOTDIR": str(home),
               "SHELL": "/bin/zsh", "PATH": "/usr/bin:/bin"}
        result = subprocess.run(["/bin/sh", str(script), "--binary", selected.name],
                                cwd=base, env=env, capture_output=True, timeout=15)
        assert result.returncode == 0, result.stderr
        assert (home / ".local/bin/kodstar").is_file()
        assert not (base / ".kodstar").exists()


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    for mode in ("success", "fail", "install-fail", "interrupt"):
        execute(binary, mode)
    local_binary(binary)
    print("Installer: success, build/install failure, interruption cleanup and local binary passed.")


if __name__ == "__main__":
    main()
