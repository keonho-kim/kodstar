# Purpose: Reproduce the pinned Kotlin grammar patch and verify generated parser provenance.
# Pattern: Grammar Build Driver.
# Usage: python3 scripts/generate_kotlin.py --tree-sitter /absolute/path/to/tree-sitter; --check verifies checked-in files.
# Related: vendor/tree-sitter-kotlin-ng/patch.json, docs/development.md.

import argparse
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parent.parent
DESTINATION = ROOT / "vendor/tree-sitter-kotlin-ng"
VERSION = "1.1.0"
REVISION = "77dd60ea0a9003ce062c9728a513ffe1aaff8c82"
CHECKSUM = "e800ebbda938acfbf224f4d2c34947a31994b1295ee6e819b65226c7b51b4450"
LICENSE_SHA = "0eea8dc45e89deeb03c7799bbbc7b4688f365fb274562f4540ecfebdea82e727"
GENERATOR = "0.26.10"
INPUTS = ["Cargo.toml", "grammar.js", "tree-sitter.json", "bindings/rust/lib.rs",
          "bindings/rust/build.rs", "src/scanner.c", "src/tree_sitter/alloc.h",
          "src/tree_sitter/array.h", "src/tree_sitter/parser.h"]
OUTPUTS = ["src/parser.c", "src/node-types.json", "src/grammar.json"]


def checksum(content):
    return hashlib.sha256(content).hexdigest()


def download(url, expected):
    with urlopen(url, timeout=30) as response:
        content = response.read(32 * 1024 * 1024 + 1)
    if len(content) > 32 * 1024 * 1024 or checksum(content) != expected:
        raise RuntimeError("pinned grammar input checksum mismatch")
    return content


def verify():
    manifest = json.loads(DESTINATION.joinpath("SOURCE.json").read_text())
    for name, expected in manifest["files"].items():
        if checksum(DESTINATION.joinpath(name).read_bytes()) != expected:
            raise RuntimeError(f"modified grammar input or output: {name}")
    print(f"Verified {len(manifest['files'])} pinned Kotlin grammar files")


def generate(binary):
    release = subprocess.run([str(binary), "--version"], capture_output=True,
                             text=True, check=True).stdout.split()
    if len(release) < 2 or release[1] != GENERATOR:
        raise RuntimeError(f"tree-sitter CLI {GENERATOR} is required")
    archive = download(f"https://static.crates.io/crates/tree-sitter-kotlin-ng/"
                       f"tree-sitter-kotlin-ng-{VERSION}.crate", CHECKSUM)
    license_url = ("https://raw.githubusercontent.com/tree-sitter-grammars/"
                   f"tree-sitter-kotlin/{REVISION}/LICENSE")
    license_text = download(license_url, LICENSE_SHA)
    patch = json.loads(DESTINATION.joinpath("patch.json").read_text())
    with tempfile.TemporaryDirectory(prefix="kodstar-kotlin-grammar-") as location:
        work = Path(location)
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
            for name in INPUTS:
                stream = package.extractfile(f"tree-sitter-kotlin-ng-{VERSION}/{name}")
                if stream is None:
                    raise RuntimeError(f"missing pinned grammar input: {name}")
                with stream:
                    content = stream.read()
                work.joinpath(name).parent.mkdir(parents=True, exist_ok=True)
                work.joinpath(name).write_bytes(content)
        grammar = work.joinpath("grammar.js").read_text()
        for change in patch["replacements"]:
            if grammar.count(change["before"]) != change["occurrences"]:
                raise RuntimeError("grammar patch does not match the pinned source")
            grammar = grammar.replace(change["before"], change["after"])
        work.joinpath("grammar.js").write_text(grammar)
        cargo = work.joinpath("Cargo.toml")
        cargo.write_text(cargo.read_text().replace('readme = "README.md"\n', ""))
        subprocess.run([str(binary), "generate", "--abi", "14"], cwd=work, check=True)
        work.joinpath("LICENSE").write_bytes(license_text)
        for name in [*INPUTS, *OUTPUTS, "LICENSE"]:
            DESTINATION.joinpath(name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(work / name, DESTINATION / name)
    files = [*INPUTS, *OUTPUTS, "LICENSE", "patch.json"]
    manifest = {
        "upstream_version": VERSION, "upstream_revision": REVISION,
        "upstream_crate_sha256": CHECKSUM, "generator": f"tree-sitter {GENERATOR}",
        "abi": 14, "manifest_adjustment": "Omit the unused upstream README reference.",
        "files": {name: checksum(DESTINATION.joinpath(name).read_bytes()) for name in files},
    }
    DESTINATION.joinpath("SOURCE.json").write_text(json.dumps(manifest, indent=2) + "\n")
    verify()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--tree-sitter", type=Path)
    options = parser.parse_args()
    if options.check:
        verify()
    elif options.tree_sitter:
        generate(options.tree_sitter.resolve(strict=True))
    else:
        parser.error("choose --check or supply --tree-sitter")


if __name__ == "__main__":
    main()
