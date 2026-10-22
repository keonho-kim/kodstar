# Purpose: Compare long-history growth and restore correctness on owned binary fixtures.
# Pattern: Benchmark Driver.
# Usage: python3 scripts/benchmarks/history.py --help on macOS after distribution builds.
# Related: scripts/benchmarks/storage.py, scripts/benchmarks/content_codec.py, docs/development.md.

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import re
import shutil
import sqlite3
import statistics
import subprocess
import tempfile
import time

from content_codec import register
from storage import source_results, measure_storage
from analysis import binary_digest


def run(binary, project, home, arguments):
    started = time.perf_counter()
    result = subprocess.run(
        ["/usr/bin/time", "-l", str(binary), *arguments, "--root", str(project),
         "--format", "json"], cwd=project, env=dict(os.environ, HOME=str(home)),
        capture_output=True, timeout=120,
    )
    seconds = time.perf_counter() - started
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors="replace"))
    output = json.loads(result.stdout)
    if not output["complete"] or output["truncated"]:
        raise RuntimeError("incomplete history workload")
    resident = re.search(rb"(\d+)\s+maximum resident set size", result.stderr)
    if resident is None:
        raise RuntimeError("missing macOS RSS")
    return {"seconds": seconds, "rss_bytes": int(resident[1])}


def all_contents(project):
    path = project / ".kodstar/repository.sqlite"
    wal = path.with_name(path.name + "-wal")
    if wal.exists() and wal.stat().st_size:
        raise RuntimeError("pending history WAL")
    result = hashlib.sha256()
    with closing(sqlite3.connect(path.as_uri() + "?mode=ro&immutable=1", uri=True)) as db:
        register(db)
        for identity, size, hash_value in db.execute("SELECT id,size,hash FROM blobs ORDER BY id"):
            result.update(json.dumps([identity, size, hash_value]).encode())
            received = 0
            for part, content in db.execute("SELECT part,content FROM chunks WHERE blob=? ORDER BY part", (identity,)):
                if part < 0:
                    raise RuntimeError("invalid chunk order")
                result.update(content)
                received += len(content)
            if received != size:
                raise RuntimeError("restored blob size mismatch")
        return result.hexdigest()


def trace(binary, project, home, length, iterations, large):
    original = random.Random(42).randbytes(length)
    path = project / "document.bin"
    path.write_bytes(original)
    first = run(binary, project, home, ["init", "--yes", "--jobs", "1"])
    initial = measure_storage(project, home)
    measurements = []
    for index in range(iterations):
        if large:
            changed = b"inserted prefix\n" * (index + 1) + original
        else:
            changed = bytearray(original)
            changed[index:index + 4] = index.to_bytes(4, "little")
        path.write_bytes(changed)
        measurements.append(run(binary, project, home, ["init", "--yes", "--jobs", "1"]))
        if (index + 1) % 100 == 0:
            print(binary.name, "versions", index + 1, flush=True)
    final = measure_storage(project, home)
    source = source_results(project)
    contents = all_contents(project)
    restore = run(binary, project, home, ["history", "restore", "c1"])
    if path.read_bytes() != original:
        raise RuntimeError("initial checkpoint restore changed original bytes")
    run(binary, project, home, ["history", "restore", f"c{iterations + 1}"])
    if path.read_bytes() != changed:
        raise RuntimeError("latest checkpoint restore changed source bytes")
    run(binary, project, home, ["doctor"])
    return {"initial": initial, "final": final, "source_results": source,
            "all_contents_sha256": contents, "init": first, "restore": restore,
            "update_median_seconds": statistics.median(item["seconds"] for item in measurements),
            "update_max_rss_bytes": max(item["rss_bytes"] for item in measurements)}


def main():
    parser = argparse.ArgumentParser(description="Compare long source histories without touching user stores")
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--small-iterations", type=int, default=1000)
    parser.add_argument("--large-iterations", type=int, default=20)
    args = parser.parse_args()
    if platform.system() != "Darwin" or not 1 <= args.small_iterations <= 8188 or args.large_iterations < 1:
        parser.error("macOS, 1..8188 small iterations and positive large iterations required")
    report = {"platform": platform.platform(), "small_iterations": args.small_iterations,
              "binary_sha256": {"baseline":binary_digest(args.baseline),"candidate":binary_digest(args.candidate)},
              "large_iterations": args.large_iterations, "runs": {}}
    with tempfile.TemporaryDirectory(prefix="kodstar-history-trace-") as directory:
        root = Path(directory)
        project, home = root / "project", root / "home"
        for name, length, count, large in [("small", 8192, args.small_iterations, False),
                                          ("large", 1024 * 1024, args.large_iterations, True)]:
            report["runs"][name] = {}
            for label, binary in [("baseline", args.baseline), ("candidate", args.candidate)]:
                for owned in [project, home]:
                    if owned.exists():
                        shutil.rmtree(owned)
                    owned.mkdir()
                report["runs"][name][label] = trace(binary.resolve(strict=True), project, home, length, count, large)
                print(name, label, "complete", flush=True)
            pair = report["runs"][name]
            for key in ["source_results", "all_contents_sha256"]:
                if pair["baseline"][key] != pair["candidate"][key]:
                    raise RuntimeError(f"history results differ: {name}, {key}")
    report["complete"] = True
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({name: {label: {"history_bytes": data["final"]["history_bytes"],
                      "median_update_seconds": data["update_median_seconds"],
                      "restore_seconds": data["restore"]["seconds"]}
                      for label, data in pair.items()} for name, pair in report["runs"].items()}, indent=2))


if __name__ == "__main__":
    main()
