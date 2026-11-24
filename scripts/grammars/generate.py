# Purpose: Reproduce canonical grammar sources, recorded patches and generated parsers.
# Pattern: Grammar Build Driver.
# Usage: python3 scripts/grammars/generate.py --check; --grammar NAME --tree-sitter PATH regenerates one.
# Related: docs/development.md, LICENSES/dependencies.json.

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT / "vendor"
OUTPUTS = ["src/parser.c", "src/node-types.json", "src/grammar.json",
           "src/tree_sitter/parser.h", "src/tree_sitter/alloc.h", "src/tree_sitter/array.h"]


def digest(content):
    return hashlib.sha256(content).hexdigest()


def relative_path(value):
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError(f"invalid grammar-relative path: {value}")
    return path


def read_manifest(directory):
    manifest = json.loads((directory / "SOURCE.json").read_text())
    if manifest.get("schema_version") != 2:
        raise ValueError(f"unsupported grammar source manifest: {directory}")
    return manifest


def verify(directory):
    manifest = read_manifest(directory)
    for name, expected in manifest["files"].items():
        path = directory / relative_path(name)
        if digest(path.read_bytes()) != expected:
            raise RuntimeError(f"modified grammar source or generated output: {path}")
    for dependency in manifest.get("dependencies", []):
        path = VENDOR / relative_path(dependency["source"])
        if digest(path.read_bytes()) != dependency["sha256"]:
            raise RuntimeError(f"modified grammar generation dependency: {path}")
    print(f"Verified {directory.name}: {len(manifest['files'])} source and generated files")


def download(entry):
    if not entry["url"].startswith("https://"):
        raise ValueError("grammar inputs require HTTPS")
    with urlopen(entry["url"], timeout=60) as response:
        content = response.read(64 * 1024 * 1024 + 1)
    if len(content) > 64 * 1024 * 1024 or digest(content) != entry["sha256"]:
        raise RuntimeError(f"pinned grammar input checksum mismatch: {entry['url']}")
    return content


def patched_inputs(directory, work, manifest):
    for entry in manifest["inputs"]:
        path = work / relative_path(entry["path"])
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(download(entry))
    patch = json.loads((directory / "patch.json").read_text())
    for change in patch.get("replacements", []):
        path = work / relative_path(change.get("file", "grammar.js"))
        content = path.read_text()
        if content.count(change["before"]) != change["occurrences"]:
            raise RuntimeError(f"grammar patch no longer matches its pinned input: {path.name}")
        path.write_text(content.replace(change["before"], change["after"]))
    cargo = work / "Cargo.toml"
    cargo.write_text(cargo.read_text().replace('readme = "README.md"\n', ""))
    for dependency in manifest.get("dependencies", []):
        source = VENDOR / relative_path(dependency["source"])
        if digest(source.read_bytes()) != dependency["sha256"]:
            raise RuntimeError(f"generation dependency differs: {source}")
        destination = work / relative_path(dependency["destination"])
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)


def generate(directory, binary):
    manifest = read_manifest(directory)
    version = subprocess.check_output([str(binary), "--version"], text=True).split()
    if len(version) < 2 or version[1] != manifest["generator"]:
        raise RuntimeError(f"tree-sitter {manifest['generator']} is required")
    with tempfile.TemporaryDirectory(prefix="kodstar-grammar-") as temporary:
        work = Path(temporary)
        patched_inputs(directory, work, manifest)
        outputs = []
        for target in manifest.get("targets", ["."]):
            location = relative_path(target)
            subprocess.run([str(binary), "generate", "--abi", str(manifest["abi"])],
                           cwd=work / location, check=True, timeout=300)
            outputs.extend(str(location / name) for name in OUTPUTS)
        files = sorted({entry["path"] for entry in manifest["inputs"]
                        if not entry.get("build_only", False)} | set(outputs))
        for name in files:
            source = work / relative_path(name)
            if not source.exists():
                raise RuntimeError(f"generator omitted a required output: {name}")
        for name in files:
            target = directory / relative_path(name)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(work / name, target)
    manifest["files"] = {name: digest((directory / name).read_bytes())
                         for name in [*files, "patch.json"]}
    (directory / "SOURCE.json").write_text(json.dumps(manifest, indent=2) + "\n")
    verify(directory)


def main():
    parser = argparse.ArgumentParser(description="Reproduce pinned upstream grammars")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--grammar", help="vendored crate name, for example tree-sitter-swift")
    parser.add_argument("--tree-sitter", type=Path)
    args = parser.parse_args()
    directories = [VENDOR / relative_path(args.grammar)] if args.grammar else sorted(
        path.parent for path in VENDOR.glob("*/SOURCE.json"))
    if args.check and args.tree_sitter:
        parser.error("choose verification or generation")
    if args.check:
        for directory in directories:
            verify(directory)
    elif args.tree_sitter and args.grammar:
        generate(directories[0], args.tree_sitter.resolve(strict=True))
    else:
        parser.error("choose --check or supply --grammar and --tree-sitter")


if __name__ == "__main__":
    main()
