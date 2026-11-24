# Purpose: Independently decode bounded lookup blocks for semantic benchmark comparisons.
# Pattern: Reference Decoder.
# Usage: analysis.py registers its SQL function on owned read-only benchmark databases.
# Related: crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs.

import json
from packed import Input, register as register_packed


def decode(payload, count):
    if not 1 <= count <= 128 or len(payload) > 1280:
        raise ValueError("invalid posting bounds")
    output, value, shift, previous = [], 0, 0, 0
    for byte in payload:
        if shift > 63 or shift == 63 and byte > 1 or shift and byte == 0:
            raise ValueError("invalid posting integer")
        value |= (byte & 127) << shift
        if byte & 128:
            shift += 7
            continue
        fact = previous + (value >> 1)
        if not previous < fact <= 2**63 - 1:
            raise ValueError("invalid posting order")
        output.append([fact, value & 1])
        previous, value, shift = fact, 0, 0
    if shift or len(output) != count:
        raise ValueError("invalid posting count")
    return json.dumps(output)


def page_rows(payload, count):
    if not 1 <= count <= 64 or len(payload) > 65536:
        raise ValueError("invalid lookup page")
    source = Input(payload)
    rows = []
    for _ in range(count):
        slot, key, part, length, size = (source.number() for _ in range(5))
        if not 0 <= slot < 64 or not 1 <= key <= 128 or not 1 <= length <= 128:
            raise ValueError("invalid lookup row")
        end = source.at + size
        if end > len(source.payload):
            raise ValueError("truncated lookup row")
        raw = source.payload[source.at:end]
        source.at = end
        rows.append([key, part, length, raw.hex()])
    source.finish()
    return json.dumps(rows)


def register(database):
    register_packed(database)
    database.create_function("kodstar_display_path", 1,
                             lambda value: value.decode("utf-8", errors="replace"), deterministic=True)
    database.create_function("kodstar_postings", 2, decode, deterministic=True)
    if database.execute("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='lookup_pages')").fetchone()[0]:
        database.create_function("reference_lookup_page", 2, page_rows, deterministic=True)
        database.executescript("""
            CREATE TEMP VIEW lookup_rows AS SELECT p.group_id route_group,
                p.group_id*128+json_extract(j.value,'$[0]') key_id,p.file_id,
                json_extract(j.value,'$[1]') part,json_extract(j.value,'$[2]') count,
                unhex(json_extract(j.value,'$[3]')) payload
                FROM lookup_pages p,json_each(reference_lookup_page(p.payload,p.count)) j;
            CREATE TEMP VIEW lookup_blocks AS SELECT key_id,file_id,part,count,payload,route_group FROM lookup_rows;
        """)
    database.executescript("""
        CREATE TEMP VIEW fact_lookups AS SELECT json_extract(j.value,'$[0]') fact_id,b.key_id,
            json_extract(j.value,'$[1]') found FROM lookup_blocks b,json_each(kodstar_postings(b.payload,b.count)) j;
        CREATE TEMP VIEW dictionary_ids AS SELECT p.id*128+json_extract(j.value,'$[0]')+1 id,
            json_extract(j.value,'$[1]') block FROM dictionary_routes p,json_each(kodstar_routes(p.payload)) j;
        CREATE TEMP VIEW relation_routes AS SELECT p.kind,p.entity,j.value block FROM relation_route_pages p,
            json_each(kodstar_relation_postings(p.payload,p.page)) j;
    """)
