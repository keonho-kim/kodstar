# Purpose: Independently restore packed dictionaries and adjacency blocks for comparison.
# Pattern: Reference Decoder.
# Usage: postings.py registers temporary logical views on owned quiescent snapshots.
# Related: scripts/benchmarks/postings.py, scripts/benchmarks/analysis.py.

import json


class Input:
    def __init__(self, payload):
        self.payload, self.at = payload, 0

    def number(self):
        value = 0
        for shift in range(0, 64, 7):
            byte = self.payload[self.at]
            self.at += 1
            if shift == 63 and byte > 1 or shift and byte == 0:
                raise ValueError("invalid packed integer")
            value |= (byte & 127) << shift
            if byte < 128:
                return value
        raise ValueError("packed integer overflow")

    def text(self):
        size = self.number()
        end = self.at + size
        if end > len(self.payload):
            raise ValueError("truncated packed string")
        result = self.payload[self.at:end]
        self.at = end
        return result

    def finish(self):
        if self.at != len(self.payload):
            raise ValueError("trailing packed bytes")


def keys(payload, count):
    if not 1 <= count <= 32:
        raise ValueError("dictionary count")
    source, previous, rows = Input(payload), b"", []
    for _ in range(count):
        identity, prefix = source.number(), source.number()
        if prefix > len(previous):
            raise ValueError("invalid dictionary prefix")
        previous = previous[:prefix] + source.text()
        rows.append([identity, previous.decode("utf-8")])
    source.finish()
    return json.dumps(rows, ensure_ascii=False)


def edges(payload, count, owner):
    if not 1 <= count <= 128:
        raise ValueError("relationship count")
    source, identity, fact, rows = Input(payload), 0, 0, []
    for _ in range(count):
        identity += source.number()
        delta = source.number()
        fact += (delta >> 1) ^ -(delta & 1)
        target, rule, reason, flags = (source.number() for _ in range(4))
        external = source.text().decode("utf-8") if flags & 1 else None
        state = ["resolved", "candidate", "ambiguous", "unresolved"][flags >> 1]
        rows.append([identity, fact, owner, target or None, external, state, rule, reason or None])
    source.finish()
    return json.dumps(rows, ensure_ascii=False)


def register(database):
    if not database.execute("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='dictionary_blocks')").fetchone()[0]:
        return
    database.create_function("reference_keys", 2, keys, deterministic=True)
    database.create_function("reference_edges", 3, edges, deterministic=True)
    # These are benchmark-only disk tables: avoid a quadratic logical-view join.
    database.execute("PRAGMA temp_store=FILE")
    database.executescript("""
        CREATE TEMP TABLE lookup_keys(id INTEGER PRIMARY KEY,kind TEXT,lookup_key TEXT);
        INSERT INTO lookup_keys SELECT json_extract(j.value,'$[0]'),b.kind,json_extract(j.value,'$[1]')
          FROM dictionary_blocks b,json_each(reference_keys(b.payload,b.count)) j;
        CREATE TEMP VIEW lookup_dependencies AS SELECT d.fact_id,k.kind,k.lookup_key,d.found FROM fact_lookups d JOIN lookup_keys k ON k.id=d.key_id;
        CREATE TEMP TABLE relationship_rows(id INTEGER PRIMARY KEY,fact_id INTEGER,source_id INTEGER,target_id INTEGER,external_key TEXT,resolution TEXT,rule_id INTEGER,reason_id INTEGER);
        INSERT INTO relationship_rows SELECT json_extract(j.value,'$[0]') id,json_extract(j.value,'$[1]') fact_id,
          json_extract(j.value,'$[2]') source_id,json_extract(j.value,'$[3]') target_id,json_extract(j.value,'$[4]') external_key,
          json_extract(j.value,'$[5]') resolution,json_extract(j.value,'$[6]') rule_id,json_extract(j.value,'$[7]') reason_id
          FROM relation_blocks b,json_each(reference_edges(b.payload,b.count,b.source_id)) j;
        CREATE INDEX reference_keys_exact ON lookup_keys(kind,lookup_key);
        CREATE INDEX reference_edges_fact ON relationship_rows(fact_id);
        CREATE TEMP VIEW relationships AS SELECT r.id,r.fact_id,r.source_id,r.target_id,r.external_key,r.resolution,
          (SELECT value FROM relation_texts WHERE id=r.rule_id) rule,(SELECT value FROM relation_texts WHERE id=r.reason_id) reason
          FROM relationship_rows r;
    """)
