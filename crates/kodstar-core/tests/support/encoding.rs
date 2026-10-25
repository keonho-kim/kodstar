// Purpose: 이력 내용을 독립적으로 복원해 업그레이드 입력 fixture를 생성.
// Pattern: Reference Decoder.
// Usage: 이력 전환 통합 테스트가 소유 DB 연결에 등록.
// Related: scripts/benchmarks/content_codec.py, crates/kodstar-core/tests/cases/history_upgrade.rs.

use rusqlite::{Connection, functions::FunctionFlags};

fn decode(payload: &[u8], codec: u8, size: usize, base: &[u8]) -> rusqlite::Result<Vec<u8>> {
    if size == 0
        || size > 65536
        || payload.len() > 65536
        || base.len() > 65536
        || codec > 2
        || (codec == 2) != !base.is_empty()
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if codec == 0 {
        return if payload.len() == size {
            Ok(payload.to_vec())
        } else {
            Err(rusqlite::Error::InvalidQuery)
        };
    }
    let base = if base.starts_with(&[0x37, 0xa4, 0x30, 0xec]) {
        &base[1..]
    } else {
        base
    };
    let mut decoder = zstd::bulk::Decompressor::with_dictionary(base)
        .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    let output = decoder
        .decompress(payload, size)
        .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    if output.len() != size {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(output)
}

pub fn register(connection: &Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "kodstar_content",
        6,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |c| {
            let base = match c.get_raw(3) {
                rusqlite::types::ValueRef::Null => Vec::new(),
                value => decode(value.as_blob()?, c.get(4)?, c.get(5)?, &[])?,
            };
            decode(c.get_raw(0).as_blob()?, c.get(1)?, c.get(2)?, &base)
        },
    )
}
