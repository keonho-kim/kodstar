# Purpose: Compare two macOS CLI binaries using isolated caches and identical indexed results.
# Pattern: Benchmark Driver.
# Usage: python3 scripts/benchmark.py --help; run after release builds without competing builds.
# Related: crates/kodstar-core/src/infrastructure/index/schema.sql, docs/development.md.

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import sqlite3
import statistics
import subprocess
import tempfile
import time


def digest(database, query):
    result = hashlib.sha256()
    for row in database.execute(query):
        result.update(json.dumps(row, ensure_ascii=False).encode("utf-8"))
        result.update(b"\n")
    return result.hexdigest()


def binary_digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def indexed_results(home):
    indexes = list((home / ".kodstar").glob("*/index.sqlite"))
    if len(indexes) != 1:
        raise RuntimeError("expected exactly one isolated benchmark index")
    # The owned child has exited and joined all work. No other actor uses this temporary cache.
    # Immutable avoids creating WAL sidecars for this checkpointed, quiescent snapshot only.
    # Never ignore pending WAL pages or use this path against a live application cache.
    wal = indexes[0].with_name(indexes[0].name + "-wal")
    if wal.exists() and wal.stat().st_size:
        raise RuntimeError("isolated index has pending WAL pages; snapshot comparison refused")
    with closing(sqlite3.connect(indexes[0].as_uri() + "?mode=ro&immutable=1", uri=True)) as database:
        database.execute("PRAGMA cache_size=-1024")
        database.execute("PRAGMA temp_store=FILE")
        symbols = digest(database, """
            SELECT hex(f.path),s.logical_key,s.name,s.qualified_name,s.kind,s.language,
                   s.signature,s.condition,o.logical_key,s.start_byte,s.end_byte,
                   s.scope_id,f.module,f.package,s.line,s.column_byte,s.end_line,s.end_column_byte,s.exported,
                   s.syntax_key,s.syntax_qualified_name,s.syntax_kind
            FROM symbols s JOIN files f ON f.id=s.file_id LEFT JOIN symbols o ON o.id=s.owner_id
            ORDER BY 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22
        """)
        relations = digest(database, """
            SELECT hex(f.path),s.logical_key,t.logical_key,r.external_key,u.relation,
                   u.expression,u.start_byte,u.end_byte,u.condition,u.type_only,
                   r.resolution,r.rule,r.reason
            FROM relationships r JOIN facts u ON u.id=r.fact_id
            JOIN files f ON f.id=u.file_id JOIN symbols s ON s.id=r.source_id
            LEFT JOIN symbols t ON t.id=r.target_id
            ORDER BY 1,2,3,4,5,6,7,8,9,10,11,12,13
        """)
        inputs = digest(database, """
            SELECT hex(path),content_hash,language,module,declared_module,package,status
            FROM files ORDER BY path
        """)
        scopes = digest(database, """
            SELECT hex(f.path),s.local_id,s.parent,s.start_byte,s.end_byte
            FROM scopes s JOIN files f ON f.id=s.file_id ORDER BY 1,2,3,4,5
        """)
        bindings = digest(database, """
            SELECT hex(f.path),b.scope_id,b.name,b.name_key,b.target_local,b.module_name,
                   b.imported_name,b.type_name,b.kind,b.start_byte,b.end_byte,b.namespace
            FROM bindings b JOIN files f ON f.id=b.file_id ORDER BY 1,2,3,4,5,6,7,8,9,10,11,12
        """)
        identities = digest(database, """
            SELECT s.syntax_key,c.logical_key FROM symbol_identities i
            JOIN symbols s ON s.id=i.symbol_id JOIN symbols c ON c.id=i.canonical_id
            ORDER BY 1,2
        """)
        lookups = digest(database, """
            SELECT hex(f.path),s.logical_key,u.scope_id,u.expression,u.relation,u.start_byte,
                   u.end_byte,d.kind,d.lookup_key,d.found
            FROM lookup_dependencies d JOIN facts u ON u.id=d.fact_id
            JOIN files f ON f.id=u.file_id JOIN symbols s ON s.id=u.source_id
            ORDER BY 1,2,3,4,5,6,7,8,9,10
        """)
        return {"inputs_sha256": inputs, "symbols_sha256": symbols, "relationships_sha256": relations,
                "scopes_sha256": scopes, "bindings_sha256": bindings,
                "identities_sha256": identities, "lookups_sha256": lookups}


def measure(binary, project, home, arguments):
    environment = dict(os.environ, HOME=str(home), USERPROFILE=str(home))
    start = time.perf_counter()
    result = subprocess.run(
        ["/usr/bin/time", "-l", str(binary), *arguments], cwd=project,
        env=environment, capture_output=True, timeout=180,
    )
    elapsed = time.perf_counter() - start
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    output = json.loads(result.stdout)
    if not output["complete"] or output["truncated"]:
        raise RuntimeError("benchmark result was incomplete")
    resident = re.search(rb"(\d+)\s+maximum resident set size", result.stderr)
    if resident is None:
        raise RuntimeError("macOS time did not report maximum RSS")
    return {"seconds": elapsed, "rss_bytes": int(resident[1]), "coverage": output["coverage"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__ or "Compare macOS KodStar release binaries")
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--project", required=True, type=Path)
    parser.add_argument("--file", required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if platform.system() != "Darwin" or args.repeats < 1:
        parser.error("macOS and a positive repeat count are required")
    project = args.project.resolve(strict=True)
    report = {"platform": platform.platform(), "machine": platform.machine(), "runs": [],
              "binary_sha256": {"baseline": binary_digest(args.baseline),
                                "candidate": binary_digest(args.candidate)}}
    expected = None
    for repeat in range(args.repeats):
        for label, binary in [("baseline", args.baseline), ("candidate", args.candidate)]:
            with tempfile.TemporaryDirectory(prefix="kodstar-benchmark-") as temporary:
                home = Path(temporary)
                common = ["--root", str(project), "--jobs", "1", "--memory-budget", "128MiB", "--format", "json"]
                run = {"label": label, "repeat": repeat}
                run["build"] = measure(binary.resolve(strict=True), project, home, ["build", *common])
                for mode, extra in [("synchronize", []), ("readonly", ["--no-refresh"])]:
                    run[mode] = measure(binary.resolve(strict=True), project, home,
                                        ["symbols", args.file, *common, *extra])
                run["indexed_results"] = indexed_results(home)
                observed = (run["build"]["coverage"], run["indexed_results"])
                report["runs"].append(run)
                if expected is None:
                    expected = observed
                elif observed != expected:
                    report["complete"] = False
                    report["mismatch"] = {
                        "label": label, "repeat": repeat,
                        "coverage_changed": observed[0] != expected[0],
                        "changed_hashes": [key for key, value in observed[1].items()
                                           if expected[1][key] != value],
                    }
                    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
                    raise RuntimeError(f"indexed results differ; comparison refused: {args.output}")
                print(label, repeat, "build", round(run["build"]["seconds"], 3), "seconds", flush=True)
    report["medians"] = {}
    report["complete"] = True
    for label in ["baseline", "candidate"]:
        runs = [run for run in report["runs"] if run["label"] == label]
        report["medians"][label] = {
            mode: {key: statistics.median(run[mode][key] for run in runs)
                   for key in ["seconds", "rss_bytes"]}
            for mode in ["build", "synchronize", "readonly"]
        }
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["medians"], indent=2))


if __name__ == "__main__":
    main()
