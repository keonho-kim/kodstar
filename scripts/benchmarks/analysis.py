# Purpose: Compare macOS CLI binaries using isolated histories, caches and identical indexed results.
# Pattern: Benchmark Driver.
# Usage: python3 scripts/benchmarks/analysis.py --help; run after release builds without competing builds.
# Related: crates/kodstar-core/src/infrastructure/index/schema.sql, docs/development.md.

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import sqlite3
import statistics
import subprocess
import tempfile
import time

from storage import measure_storage, source_results
from postings import register


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
        register(database)
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
            SELECT hex(path),CASE WHEN status IN ('unsupported','skipped') THEN '' WHEN typeof(content_hash)='blob' THEN lower(hex(content_hash)) ELSE content_hash END,language,module,declared_module,package,status
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
        lookup_sql = Path(__file__).with_name("lookups.sql").read_text()
        return {"inputs_sha256": inputs, "symbols_sha256": symbols,
                "relationships_sha256": relations, "scopes_sha256": scopes,
                "bindings_sha256": bindings, "identities_sha256": identities,
                "lookups_sha256": digest(database, lookup_sql),
                "native_analysis_sha256": digest(database, "SELECT hex(f.path),n.status,n.detail FROM native_analysis n JOIN files f ON f.id=n.file_id ORDER BY 1,2,3"),
                "native_environment_sha256": digest(database, "SELECT hex(f.path),hex(n.environment) FROM native_analysis n JOIN files f ON f.id=n.file_id ORDER BY 1,2"),
                "native_inputs_sha256": digest(database, "SELECT hex(f.path),n.path,n.kind,hex(n.hash) FROM native_inputs n JOIN files f ON f.id=n.file_id ORDER BY 1,2,3,4")}


def measure(binary, project, home, arguments):
    environment = dict(os.environ, HOME=str(home))
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
    return {"seconds": elapsed, "rss_bytes": int(resident[1]), "coverage": output["coverage"],
            **measure_storage(project, home)}


def prepare_mutations(project):
    directory = project / "kodstar_benchmark"
    if directory.exists():
        raise RuntimeError("fixture already contains kodstar_benchmark")
    directory.mkdir()
    source = ("package kodstar_benchmark\n"
              "func KodstarBenchFirst() {}\nfunc KodstarBenchOther() {}\n"
              "func KodstarBenchCaller() { KodstarBenchFirst() }\n")
    (directory / "benchmark.go").write_text(source)
    (directory / "notes.md").write_text("Original documentation.\n")
    return directory, source


def mutation_runs(binary, project, home, common, fixture):
    directory, source = fixture
    for name in ["notes", "body", "add", "delete"]:
        if name == "notes":
            (directory / "notes.md").write_text("Updated documentation.\n")
        elif name == "body":
            (directory / "benchmark.go").write_text(source.replace("{ KodstarBenchFirst() }", "{ KodstarBenchOther() }"))
        elif name == "add":
            (directory / "benchmark.go").write_text(source.replace("{ KodstarBenchFirst() }", "{ KodstarBenchAdded() }"))
            (directory / "added.go").write_text("package kodstar_benchmark\nfunc KodstarBenchAdded() {}\n")
        else:
            (directory / "added.go").unlink()
        result = measure(binary, project, home, ["symbols", str(directory / "benchmark.go"), *common])
        result["indexed_results"] = indexed_results(home)
        result["source_results"] = source_results(project)
        yield name, result


def semantic_result(result, lookup_contract_change=False, native_engine_change=False):
    coverage = {key: value for key, value in result["coverage"].items() if key != "reused_files"}
    indexed = dict(result["indexed_results"])
    if lookup_contract_change:
        indexed.pop("lookups_sha256")
    if native_engine_change:
        indexed.pop("native_environment_sha256")
    return coverage, indexed, result["source_results"]


def edit_run(binary, project, home, common, filename, suffix=b"\n", replacement=None):
    source = (project / filename).resolve(strict=True)
    if not source.is_relative_to(project.resolve()) or not source.is_file():
        raise RuntimeError("newline probe must refer to a file inside the owned project copy")
    if replacement:
        before, after = (value.encode("utf-8") for value in replacement)
        content = source.read_bytes()
        if not before or before not in content or before == after:
            raise RuntimeError("body replacement must change an existing nonempty source fragment")
        source.write_bytes(content.replace(before, after, 1))
    else:
        with source.open("ab") as stream:
            stream.write(suffix)
    results = {}
    for mode, arguments in [("incremental", ["symbols", filename, "--all", *common]),
                            ("full", ["init", "--yes", "--full", *common])]:
        result = measure(binary, project, home, arguments)
        result["indexed_results"] = indexed_results(home)
        result["source_results"] = source_results(project)
        results[mode] = result
    if semantic_result(results["incremental"]) != semantic_result(results["full"]):
        raise RuntimeError("edited input incremental/full results differ")
    return results


def declaration_suffix(filename):
    name = "kodstar_benchmark_added"
    extension = Path(filename).suffix
    patterns = {
        ".py": f"\ndef {name}(): pass\n",
        ".go": f"\nfunc {name}() {{}}\n",
        ".rs": f"\npub fn {name}() {{}}\n",
        ".js": f"\nexport function {name}() {{}}\n",
        ".ts": f"\nexport function {name}() {{}}\n",
        ".zig": f"\npub fn {name}() void {{}}\n",
    }
    if extension not in patterns:
        raise RuntimeError("declaration probe requires a Python, Go, Rust, JavaScript, TypeScript or Zig file")
    return patterns[extension].encode("utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__ or "Compare macOS KodStar release binaries")
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--project", required=True, type=Path)
    parser.add_argument("--file", required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--native-engine-change", action="store_true", help="record changed native fingerprints while comparing all semantic results and diagnostics")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--mutations", action="store_true", help="also compare a Go probe's notes, body, additions and deletions")
    parser.add_argument("--newline-edit", action="store_true", help="compare an appended newline and verify incremental/full equivalence")
    parser.add_argument("--body-replacement", nargs=2, metavar=("OLD", "NEW"), help="replace one source fragment and verify incremental/full equivalence")
    parser.add_argument("--declaration-edit", action="store_true", help="append a function and verify incremental/full equivalence")
    parser.add_argument("--lookup-contract-change", action="store_true",
                        help="compare nine output/source hashes across binaries; still require all ten for edited incremental/full")
    args = parser.parse_args()
    if args.lookup_contract_change and not (args.newline_edit or args.declaration_edit or args.body_replacement):
        parser.error("--lookup-contract-change requires an edit probe to verify the new dependency contract")
    if platform.system() != "Darwin" or args.repeats < 1 or args.jobs < 1:
        parser.error("macOS and positive repeat and worker counts are required")
    project = args.project.resolve(strict=True)
    report = {"platform": platform.platform(), "machine": platform.machine(), "jobs": args.jobs, "runs": [],
              "lookup_contract_change": args.lookup_contract_change,
              "native_engine_change": args.native_engine_change,
              "binary_sha256": {"baseline": binary_digest(args.baseline),
                                "candidate": binary_digest(args.candidate)}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    expected = None
    expected_mutations = {}
    expected_edits = {}
    report["launch_warmup_seconds"] = {}
    for label in ("baseline", "candidate"):
        started = time.perf_counter()
        subprocess.run([str(getattr(args, label).resolve(strict=True)), "--version"], check=True, capture_output=True)
        report["launch_warmup_seconds"][label] = time.perf_counter() - started
    # Package and object identities include the canonical root. Reset one owned path
    # rather than comparing opaque identities from unrelated temporary roots.
    with tempfile.TemporaryDirectory(prefix="kodstar-benchmark-input-") as sandbox:
        working = Path(sandbox) / "project"
        for repeat in range(args.repeats):
            ordered = [("baseline", args.baseline), ("candidate", args.candidate)]
            if repeat % 2:
                ordered.reverse()
            for label, binary in ordered:
                with tempfile.TemporaryDirectory(prefix="kodstar-benchmark-") as temporary:
                    home = Path(temporary) / "home"
                    home.mkdir()
                    if working.exists():
                        shutil.rmtree(working)
                    shutil.copytree(project, working, symlinks=True,
                                    ignore=shutil.ignore_patterns(".kodstar"))
                    fixture = prepare_mutations(working) if args.mutations else None
                    common = ["--root", str(working), "--jobs", str(args.jobs), "--memory-budget", "128MiB", "--format", "json"]
                    run = {"label": label, "repeat": repeat}
                    run["init"] = measure(binary.resolve(strict=True), working, home, ["init", "--yes", *common])
                    for mode, extra in [("synchronize", []), ("readonly", ["--no-refresh"])]:
                        run[mode] = measure(binary.resolve(strict=True), working, home,
                                            ["symbols", args.file, "--all", *common, *extra])
                    run["indexed_results"] = indexed_results(home)
                    run["source_results"] = source_results(working)
                    compared = {**run["indexed_results"], **run["source_results"]}
                    if args.native_engine_change:
                        compared.pop("native_environment_sha256")
                    if args.lookup_contract_change:
                        compared.pop("lookups_sha256")
                    observed = (run["init"]["coverage"], compared)
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
                    print(label, repeat, "init", round(run["init"]["seconds"], 3), "seconds", flush=True)
                    for name, enabled in [("newline", args.newline_edit), ("body", args.body_replacement), ("declaration", args.declaration_edit)]:
                        if not enabled:
                            continue
                        suffix = declaration_suffix(args.file) if name == "declaration" else b"\n"
                        run[name] = edit_run(binary.resolve(strict=True), working, home, common, args.file, suffix,
                                             args.body_replacement if name == "body" else None)
                        observed_edit = semantic_result(run[name]["incremental"], args.lookup_contract_change, args.native_engine_change)
                        if name not in expected_edits:
                            expected_edits[name] = observed_edit
                        elif observed_edit != expected_edits[name]:
                            raise RuntimeError(f"baseline/candidate {name} results differ")
                        print(label, repeat, name, round(run[name]["incremental"]["seconds"], 3), "seconds", flush=True)
                    if fixture:
                        run["mutations"] = {}
                        for name, result in mutation_runs(binary.resolve(strict=True), working, home, common, fixture):
                            run["mutations"][name] = result
                            observed_mutation = semantic_result(result, args.lookup_contract_change, args.native_engine_change)
                            if name not in expected_mutations:
                                expected_mutations[name] = observed_mutation
                            elif observed_mutation != expected_mutations[name]:
                                report["complete"] = False
                                report["mismatch"] = {"label":label,"repeat":repeat,"mutation":name}
                                args.output.write_text(json.dumps(report, indent=2) + "\n")
                                raise RuntimeError(f"mutation results differ: {name}")
                            print(label, repeat, name, round(result["seconds"], 3), "seconds", flush=True)
    report["medians"] = {}
    report["complete"] = True
    for label in ["baseline", "candidate"]:
        runs = [run for run in report["runs"] if run["label"] == label]
        report["medians"][label] = {
            mode: {key: statistics.median(run[mode][key] for run in runs)
                   for key in ["seconds", "rss_bytes", "history_bytes", "cache_bytes",
                               "history_allocated_bytes", "cache_allocated_bytes"]}
            for mode in ["init", "synchronize", "readonly"]
        }
        if args.mutations:
            report["medians"][label]["mutations"] = {
                name: {key: statistics.median(run["mutations"][name][key] for run in runs)
                       for key in ["seconds", "rss_bytes", "history_bytes", "cache_bytes"]}
                for name in expected_mutations
            }
        for name in expected_edits:
            report["medians"][label][name] = {
                mode: {key: statistics.median(run[name][mode][key] for run in runs)
                       for key in ["seconds", "rss_bytes", "history_bytes", "cache_bytes"]}
                for mode in ["incremental", "full"]
            }
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["medians"], indent=2))


if __name__ == "__main__":
    main()
