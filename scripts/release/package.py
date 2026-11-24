# Purpose: Package and exercise a distributable KodStar binary on its own platform.
# Pattern: Release Check.
# Usage: python3 -B scripts/release/package.py BINARY ARCHIVE.
# Related: .github/workflows/release.yml, install.sh, crates/kodstar-core/tests/cases/clang.rs.

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = """\
#include <stddef.h>
int pick(int value) { return value; }
double pick(double value) { return value; }
int caller() { return pick(1); }
"""


def run(command, cwd, env):
    result = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True, timeout=60)
    if result.returncode:
        raise RuntimeError(f"{Path(command[0]).name} {command[1]} exited {result.returncode}\n{result.stderr}")
    return result.stdout


def verify(distribution, temporary):
    home = temporary / "home"
    project = temporary / "project"
    scratch = temporary / "scratch"
    for directory in (home, project, scratch):
        directory.mkdir()
    env = dict(
        os.environ,
        HOME=str(home),
        SHELL="/bin/sh",
        ZDOTDIR=str(home),
        XDG_CONFIG_HOME=str(home / ".config"),
        TMPDIR=str(scratch),
    )
    run([str(distribution / "install.sh"), "--binary", str(distribution / "kodstar")], project, env)
    installed = home / ".local/bin/kodstar"
    if not (home / ".kodstar/install.json").is_file():
        raise RuntimeError("Installation receipt is missing")
    version = run([str(installed), "--version"], project, env).strip()

    (project / "main.cpp").write_text(FIXTURE, encoding="utf-8")
    command = [str(installed), "--verbose", "false"]
    report = json.loads(run([*command, "init", "--yes", "--format", "json"], project, env))
    if not report["complete"]:
        raise RuntimeError("Initial fixture analysis was incomplete")
    relations = json.loads(
        run([*command, "deps", "main.cpp", "caller", "--no-refresh", "--format", "json"], project, env)
    )
    if not relations["complete"] or not any(
        row.get("relation") == "calls"
        and row.get("rule") == "clang_semantic_call"
        and (row.get("target") or {}).get("name") == "pick"
        for row in relations["results"]
    ):
        raise RuntimeError("Packaged binary did not resolve the Clang overload")
    print(f"Verified {version}: packaged installation and embedded Clang lookup passed.")


def main():
    parser = argparse.ArgumentParser(description="Verify and package a KodStar release binary.")
    parser.add_argument("binary", type=Path)
    parser.add_argument("archive", type=Path)
    arguments = parser.parse_args()
    binary = arguments.binary.resolve(strict=True)
    archive = arguments.archive.resolve()
    archive.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="kodstar-release-smoke-") as name:
        temporary = Path(name)
        distribution = temporary / "distribution"
        distribution.mkdir()
        shutil.copy2(binary, distribution / "kodstar")
        shutil.copy2(ROOT / "install.sh", distribution / "install.sh")
        shutil.copytree(ROOT / "LICENSES", distribution / "LICENSES")
        verify(distribution, temporary)
        with tarfile.open(archive, "x:gz", compresslevel=6) as packaged:
            for item in sorted(distribution.iterdir()):
                packaged.add(item, arcname=item.name)


if __name__ == "__main__":
    main()
