# Purpose: Compare graph query binaries with identical saved inputs, ordering and summaries.
# Pattern: Benchmark Driver.
# Usage: python3 -B scripts/benchmarks/graph.py --baseline PATH --candidate PATH --output PATH.
# Related: scripts/benchmarks/analysis.py, docs/development.md.

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import tempfile
import time

from analysis import binary_digest, indexed_results
from storage import measure_storage, source_results


def fixture(project, nodes, width):
    project.mkdir()
    (project / "go.mod").write_text("module example.test/graph\n\ngo 1.24\n")
    header = ("// Purpose: Generated bounded graph benchmark input.\n"
              "// Pattern: Fixture.\n// Usage: Parsed without execution.\n"
              "// Related: scripts/benchmarks/graph.py.\npackage graph\n")
    edges = 0
    for start in range(0, nodes, 100):
        lines = []
        for node in range(start, min(start + 100, nodes)):
            if width == 0:
                targets = range(1, nodes) if node == 0 else range(0)
            elif width == -1:
                targets = range(1) if node else range(0)
            else:
                targets = range(node + 1, min(node + width + 1, nodes))
            calls = "; ".join(f"n{target:04}()" for target in targets)
            edges += len(targets)
            lines.append(f"func n{node:04}() {{ {calls} }}\n")
        (project / f"part{start // 100:03}.go").write_text(header + "".join(lines))
    return edges


def semantic(result):
    # Metadata contains times/cache paths; rows and traversal summaries are the public comparison.
    return {key: value for key, value in result.items()
            if key in ("rows", "results", "complete", "truncated", "result_count",
                       "visited_count", "unresolved_count", "reasons", "summary", "coverage")}


def run(binary, project, home, arguments):
    start = time.perf_counter()
    process = subprocess.run(["/usr/bin/time", "-l", str(binary), *arguments],
                             cwd=project, env=dict(os.environ, HOME=str(home)),
                             capture_output=True, timeout=180)
    seconds = time.perf_counter() - start
    if process.returncode not in (0, 3):
        raise RuntimeError(process.stderr.decode(errors="replace"))
    result = json.loads(process.stdout)
    if not any(key in result for key in ("rows", "results")):
        raise RuntimeError("query did not return a recognized row envelope")
    resident = re.search(rb"(\d+)\s+maximum resident set size", process.stderr)
    if resident is None:
        raise RuntimeError("macOS time did not report RSS")
    digest = hashlib.sha256(json.dumps(semantic(result), sort_keys=True).encode()).hexdigest()
    return {"seconds": seconds, "rss_bytes": int(resident[1]), "output_sha256": digest,
            "result_count": result["result_count"], "visited_count": result["visited_count"],
            "complete": result["complete"], "truncated": result["truncated"],
            "exit_code": process.returncode}


def compare(arguments, directory, name, nodes, width, query):
    project = directory / name / "project"
    labels = ("baseline", "candidate")
    homes = {label: directory / name / (label if arguments.independent_indexes else "shared") / "home" for label in labels}
    for home in set(homes.values()):
        home.mkdir(parents=True)
    edges = fixture(project, nodes, width)
    common = ["--root", str(project), "--format", "json", "--all", "--no-refresh"]
    for label in (labels if arguments.independent_indexes else ("baseline",)):
        setup = subprocess.run([str(getattr(arguments,label)), "init", str(project), "--yes", "--jobs", "1", "--format", "json"],
                               cwd=project, env=dict(os.environ, HOME=str(homes[label])),
                               capture_output=True, timeout=180)
        if setup.returncode:
            raise RuntimeError(setup.stderr.decode(errors="replace"))
    before = {label: {**indexed_results(homes[label]), **source_results(project)} for label in labels}
    if before["baseline"] != before["candidate"]:
        raise RuntimeError(f"{name}: independently built graph inputs differ")
    storage = {label: measure_storage(project,home) for label,home in homes.items()}
    rows, warm_storage = [], {}
    for label in labels:
        run(getattr(arguments, label), project, homes[label], query + common)
        warm_storage[label] = measure_storage(project, homes[label])
    for repeat in range(arguments.repeats):
        ordered = labels if repeat % 2 == 0 else tuple(reversed(labels))
        for label in ordered:
            row = run(getattr(arguments, label), project, homes[label], query + common)
            rows.append({"label": label, "repeat": repeat, **row})
    identical = len({row["output_sha256"] for row in rows}) == 1
    if query[0] == "related" and "--max-nodes" not in query:
        if any(row["result_count"] != edges or row["visited_count"] != nodes for row in rows):
            raise RuntimeError(f"{name}: fixture traversal did not cover the complete graph")
    after = {label: {**indexed_results(home), **source_results(project)} for label,home in homes.items()}
    if not identical or before != after:
        raise RuntimeError(f"{name}: output/order/summary or stored input mismatch")
    if not arguments.independent_indexes:
        for key in ("history_bytes", "cache_bytes"):
            if warm_storage["baseline"][key] != warm_storage["candidate"][key]:
                raise RuntimeError(f"{name}: candidate changed the warmed store footprint")
    medians = {label: {key: statistics.median(row[key] for row in rows if row["label"] == label)
                       for key in ("seconds", "rss_bytes")} for label in labels}
    return {"nodes": nodes, "edges": edges, "rows": rows, "medians": medians,
            "identical_output": identical, "index_unchanged": True, "storage": storage,
            "independent_indexes": arguments.independent_indexes, "warmed_storage": warm_storage,
            "storage_after": {label: measure_storage(project,home) for label,home in homes.items()},
            "time_reduction_percent": 100 * (1 - medians["candidate"]["seconds"] / medians["baseline"]["seconds"])}


def main():
    parser = argparse.ArgumentParser(description="Compare complete graph query outputs and resources on macOS.")
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--independent-indexes", action="store_true", help="build each storage format separately at the same source root")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if platform.system() != "Darwin" or args.repeats < 1:
        parser.error("requires macOS /usr/bin/time -l and positive repeats")
    args.baseline, args.candidate = args.baseline.resolve(), args.candidate.resolve()
    report = {"platform": platform.platform(), "binary": {
        label: {"sha256": binary_digest(getattr(args, label)), "bytes": getattr(args, label).stat().st_size}
        for label in ("baseline", "candidate")}, "cases": {}}
    cases = [
        ("related", 1500, 2, ["related", "part000.go", "n0000", "--direction", "out", "--transitive", "--sites"]),
        ("shared_path", 500, 24, ["path", "part000.go::n0000", "part004.go::n0499", "--direction", "out"]),
        ("bounded", 500, 24, ["related", "part000.go", "n0000", "--direction", "both", "--transitive", "--max-nodes", "83"]),
        ("wide_out", 1600, 0, ["related", "part000.go", "n0000", "--direction", "out", "--transitive", "--sites"]),
        ("wide_in", 1600, -1, ["related", "part000.go", "n0000", "--direction", "in", "--transitive", "--sites"]),
    ]
    with tempfile.TemporaryDirectory(prefix="kodstar-graph-benchmark-") as temporary:
        for name, nodes, width, query in cases:
            report["cases"][name] = compare(args, Path(temporary), name, nodes, width, query)
            print(name, report["cases"][name]["medians"], flush=True)
    report["temporary_directories_removed"] = True
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
