# Purpose: Reconstruct bounded history payloads using the standard Zstd library.
# Pattern: Reference Decoder Adapter.
# Usage: storage.py and history.py register this on owned benchmark databases.
# Related: scripts/benchmarks/prior_content_codec.py, crates/kodstar-core/src/infrastructure/history/content/encoding.rs.

import ctypes
import ctypes.util
from functools import lru_cache
import os


@lru_cache(maxsize=1)
def library():
    path = os.environ.get("KODSTAR_ZSTD_LIBRARY") or ctypes.util.find_library("zstd")
    if not path:
        raise RuntimeError("libzstd is required for history benchmarks; set KODSTAR_ZSTD_LIBRARY")
    lib = ctypes.CDLL(path)
    signatures = {
        "ZSTD_createDCtx": ([], ctypes.c_void_p),
        "ZSTD_freeDCtx": ([ctypes.c_void_p], ctypes.c_size_t),
        "ZSTD_isError": ([ctypes.c_size_t], ctypes.c_uint),
        "ZSTD_getFrameContentSize": ([ctypes.c_void_p, ctypes.c_size_t], ctypes.c_ulonglong),
        "ZSTD_findFrameCompressedSize": ([ctypes.c_void_p, ctypes.c_size_t], ctypes.c_size_t),
        "ZSTD_decompress_usingDict": ([ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                      ctypes.c_void_p, ctypes.c_size_t,
                                      ctypes.c_void_p, ctypes.c_size_t], ctypes.c_size_t),
    }
    for name, (arguments, result) in signatures.items():
        function = getattr(lib, name)
        function.argtypes, function.restype = arguments, result
    return lib


def decode(payload, codec, size, base=b""):
    if (not 1 <= size <= 65536 or len(payload) > 65536 or len(base) > 65536
            or codec not in (0, 1, 2) or (codec == 2) != bool(base)):
        raise ValueError("invalid history encoding bounds or base")
    if codec == 0:
        if len(payload) != size:
            raise ValueError("invalid raw size")
        return payload
    lib = library()
    if (lib.ZSTD_getFrameContentSize(payload, len(payload)) != size
            or lib.ZSTD_findFrameCompressedSize(payload, len(payload)) != len(payload)):
        raise ValueError("invalid Zstd frame or declared size")
    # The stored base is source content, never a trained dictionary header.
    if base.startswith(bytes([0x37, 0xa4, 0x30, 0xec])):
        base = base[1:]
    output = ctypes.create_string_buffer(size)
    context = lib.ZSTD_createDCtx()
    if not context:
        raise MemoryError("cannot create Zstd reference decoder")
    try:
        received = lib.ZSTD_decompress_usingDict(context, output, size, payload,
                                                len(payload), base, len(base))
        if lib.ZSTD_isError(received) or received != size:
            raise ValueError("invalid Zstd history payload")
        return output.raw
    finally:
        lib.ZSTD_freeDCtx(context)


def register(database):
    version = database.execute("PRAGMA user_version").fetchone()[0]
    application = database.execute("PRAGMA application_id").fetchone()[0]
    if application != 0x4b534852:
        return
    if version not in (1, 2, 3, 4, 5):
        raise ValueError("unsupported benchmark history format")
    decoder = decode
    if version <= 4:
        from prior_content_codec import decode as decoder

    def content(payload, codec, size, base_payload, base_codec, base_size):
        base = b"" if base_payload is None else decoder(base_payload, base_codec, base_size)
        return decoder(payload, codec, size, base)
    database.create_function("kodstar_content", 6, content, deterministic=True)
