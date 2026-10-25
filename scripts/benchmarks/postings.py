# Purpose: Independently decode bounded lookup blocks for semantic benchmark comparisons.
# Pattern: Reference Decoder.
# Usage: analysis.py registers its SQL function on owned read-only benchmark databases.
# Related: crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs.

import json
from packed import register as register_packed


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


def register(database):
    register_packed(database)
    database.create_function("kodstar_display_path", 1,
                             lambda value: value.decode("utf-8", errors="replace"), deterministic=True)
    database.create_function("kodstar_postings", 2, decode, deterministic=True)
