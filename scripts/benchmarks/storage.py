# Purpose: Measure quiescent benchmark stores without retaining source contents.
# Pattern: Measurement Adapter.
# Usage: analysis.py records disk and SQLite statistics after each owned child exits.
# Related: scripts/benchmarks/analysis.py, docs/development.md.

from contextlib import closing
import hashlib
import json
import sqlite3
from content_codec import register


def footprint(directory):
    logical = allocated = 0
    if directory.exists():
        for path in directory.rglob("*"):
            if path.is_file() and not path.is_symlink():
                metadata = path.stat()
                logical += metadata.st_size
                allocated += metadata.st_blocks * 512
    return logical, allocated


def database_stats(path, history=False):
    wal = path.with_name(path.name + "-wal")
    if wal.exists() and wal.stat().st_size:
        raise RuntimeError("pending WAL in an owned benchmark store; measurement refused")
    with closing(sqlite3.connect(path.as_uri() + "?mode=ro&immutable=1", uri=True)) as database:
        register(database)
        database.execute("PRAGMA cache_size=-1024")
        database.execute("PRAGMA temp_store=FILE")
        page_size = database.execute("PRAGMA page_size").fetchone()[0]
        pages = database.execute("PRAGMA page_count").fetchone()[0]
        free = database.execute("PRAGMA freelist_count").fetchone()[0]
        result = {"page_bytes": page_size, "pages": pages, "free_pages": free,
                  "live_page_bytes": (pages - free) * page_size}
        try:
            result["table_bytes"] = dict(database.execute(
                "SELECT name,sum(pgsize) FROM dbstat GROUP BY name ORDER BY name"))
        except sqlite3.OperationalError as error:
            if "no such table: dbstat" not in str(error):
                raise
            result["table_bytes"] = None
        if history:
            shared = database.execute("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='chunk_data')").fetchone()[0]
            result.update({"checkpoints": database.execute("SELECT count(*) FROM checkpoints").fetchone()[0],
                           "blobs": database.execute("SELECT count(*) FROM blobs").fetchone()[0],
                           "blob_bytes": database.execute("SELECT coalesce(sum(size),0) FROM blobs").fetchone()[0],
                           "chunk_bytes": database.execute("SELECT coalesce(sum(length(content)),0) FROM chunks").fetchone()[0],
                           "stored_chunk_bytes": database.execute("SELECT coalesce(sum(length(content)),0) FROM " + ("chunk_data" if shared else "chunks")).fetchone()[0]})
        return result


def measure_storage(project, home):
    history = project / ".kodstar"
    cache = home / ".kodstar"
    history_bytes, history_allocated = footprint(history)
    cache_bytes, cache_allocated = footprint(cache)
    detail = {"history": database_stats(history / "repository.sqlite", history=True),
              "indexes": {path.name: database_stats(path)
                          for path in sorted(cache.glob("*/*.sqlite"))}}
    return {"history_bytes": history_bytes, "cache_bytes": cache_bytes,
            "history_allocated_bytes": history_allocated,
            "cache_allocated_bytes": cache_allocated, "storage": detail}


def source_results(project):
    path = project / ".kodstar/repository.sqlite"
    wal = path.with_name(path.name + "-wal")
    if wal.exists() and wal.stat().st_size:
        raise RuntimeError("pending source WAL; comparison refused")
    manifest, contents, lineage = (hashlib.sha256() for _ in range(3))
    with closing(sqlite3.connect(path.as_uri() + "?mode=ro&immutable=1", uri=True)) as database:
        register(database)
        database.execute("PRAGMA cache_size=-1024")
        database.execute("PRAGMA temp_store=FILE")
        for row in database.execute("SELECT hex(w.path),CASE WHEN typeof(b.hash)='blob' THEN lower(hex(b.hash)) ELSE b.hash END,b.size,w.mode,w.kind FROM working w JOIN blobs b ON b.id=w.blob ORDER BY w.path"):
            manifest.update(json.dumps(row, ensure_ascii=False).encode("utf-8") + b"\n")
        previous, expected, received = None, 0, 0
        for name, size, chunk in database.execute("SELECT w.path,b.size,c.content FROM working w JOIN blobs b ON b.id=w.blob LEFT JOIN chunks c ON c.blob=b.id ORDER BY w.path,c.part"):
            if name != previous:
                if received != expected:
                    raise RuntimeError("stored source length mismatch")
                contents.update(len(name).to_bytes(8, "little") + name + size.to_bytes(8, "little"))
                previous, expected, received = name, size, 0
            if chunk is not None:
                contents.update(chunk)
                received += len(chunk)
        if received != expected:
            raise RuntimeError("stored source length mismatch")
        for query in ["SELECT id,parent,changed_files,tracked_files FROM checkpoints ORDER BY id",
                      "SELECT name,checkpoint FROM branches ORDER BY name",
                      "SELECT name,checkpoint FROM names ORDER BY name"]:
            lineage.update(query.encode("utf-8") + b"\n")
            for row in database.execute(query):
                lineage.update(json.dumps(row).encode("utf-8") + b"\n")
    return {"source_manifest_sha256": manifest.hexdigest(),
            "source_bytes_sha256": contents.hexdigest(), "lineage_sha256": lineage.hexdigest()}
