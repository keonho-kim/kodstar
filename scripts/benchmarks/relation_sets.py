# Purpose: Independently decode shared relationship targets and their per-use associations.
# Pattern: Reference Decoder.
# Usage: packed.py selects this reader for indexes with a relation_sets table.
# Related: scripts/benchmarks/packed.py, scripts/benchmarks/analysis.py.

from contextlib import closing
import json

from packed import Input


def target(reader):
    identity, rule, reason, flags = (reader.number() for _ in range(4))
    if flags > 7 or rule == 0:
        raise ValueError("invalid relationship target")
    external = reader.text().decode("utf-8") if flags & 1 else None
    state = ["resolved", "candidate", "ambiguous", "unresolved"][flags >> 1]
    return [identity or None, external, state, rule, reason or None]


def edges(database, payload, count, owner):
    if not 1 <= count <= 128:
        raise ValueError("relationship count")
    source, identity, fact, associations = Input(payload), 0, 0, []
    for _ in range(count):
        gap = source.number()
        identity += gap
        delta = source.number()
        fact += (delta >> 1) ^ -(delta & 1)
        if gap == 0 or identity > 2**63 - 1 or fact < 1:
            raise ValueError("invalid relationship association")
        associations.append([identity, fact, owner])
    rows = []
    while len(rows) < count:
        size, shared = source.number(), source.number()
        if size < 1 or size > count - len(rows):
            raise ValueError("relationship group count")
        if shared:
            with closing(database.execute("SELECT count,payload FROM relation_sets WHERE id=?", (shared,))) as cursor:
                stored = cursor.fetchone()
            if stored is None or stored[0] != size:
                raise ValueError("missing or mismatched relationship target set")
            reader = Input(stored[1])
        else:
            reader = source
        for _ in range(size):
            rows.append(associations[len(rows)] + target(reader))
        if shared:
            reader.finish()
    source.finish()
    return json.dumps(rows, ensure_ascii=False)


def postings(payload, page):
    source, slot, result = Input(payload), -1, []
    while source.at < len(payload):
        gap = source.number()
        slot += gap
        identity = page * 1024 + slot + 1
        if gap == 0 or slot >= 1024 or not 0 < identity <= 2**63 - 1:
            raise ValueError("invalid relationship route page")
        result.append(identity)
    source.finish()
    return json.dumps(result)
