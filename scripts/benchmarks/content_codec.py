# Purpose: Independently reconstruct bounded source payloads for history comparisons.
# Pattern: Reference Decoder.
# Usage: storage.py registers its SQL decoder on owned history databases.
# Related: crates/kodstar-core/src/infrastructure/history/content/encoding.rs.

def decode(payload, codec, size, base=b""):
    if not 1 <= size <= 65536 or len(payload) > 65536 or len(base) > 65536:
        raise ValueError("invalid history encoding bounds")
    if codec not in (0, 1, 2) or (codec == 2) != bool(base):
        raise ValueError("invalid history encoding base")
    if codec == 0:
        if len(payload) != size:
            raise ValueError("invalid raw size")
        return payload
    output, offset = bytearray(), 0

    def integer():
        nonlocal offset
        value = shift = 0
        while offset < len(payload) and shift <= 63:
            byte = payload[offset]
            offset += 1
            if shift == 63 and byte > 1 or shift and byte == 0:
                raise ValueError("invalid encoded integer")
            value |= (byte & 127) << shift
            if byte < 128:
                return value
            shift += 7
        raise ValueError("truncated encoded integer")

    while offset < len(payload):
        token = integer()
        length = (token >> 1) + (4 if token & 1 else 1)
        if length > size - len(output):
            raise ValueError("decoded size exceeded")
        if token & 1:
            source = integer()
            if source >= len(base) + len(output):
                raise ValueError("invalid encoded copy")
            for at in range(source, source + length):
                output.append(base[at] if at < len(base) else output[at - len(base)])
        else:
            if offset + length > len(payload):
                raise ValueError("truncated literal")
            output.extend(payload[offset:offset + length])
            offset += length
    if len(output) != size:
        raise ValueError("decoded size mismatch")
    return bytes(output)


def register(database):
    def content(payload, codec, size, base_payload, base_codec, base_size):
        base = b"" if base_payload is None else decode(base_payload, base_codec, base_size)
        return decode(payload, codec, size, base)
    database.create_function("kodstar_content", 6, content, deterministic=True)
