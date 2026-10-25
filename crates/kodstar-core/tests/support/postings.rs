// Purpose: 압축 lookup의 독립 기준 decoder를 통합 테스트 SQL에 제공.
// Pattern: Reference Decoder.
// Usage: snapshot과 publication rollback 사례가 연결에 등록.
// Related: scripts/benchmarks/postings.py, crates/kodstar-core/tests/support/snapshot.rs.

pub fn register(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    super::packed::register(connection)?;
    display_path(connection)?;
    connection.create_scalar_function(
        "kodstar_digest",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            use rusqlite::types::ValueRef;
            match ctx.get_raw(0) {
                ValueRef::Null => Ok(None),
                ValueRef::Blob(v) if v.len() == 32 => Ok(Some(v.to_vec())),
                ValueRef::Text(v) => {
                    let text = std::str::from_utf8(v).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    let bytes =
                        blake3::Hash::from_hex(text).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(Some(bytes.as_bytes().to_vec()))
                }
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        },
    )?;
    connection.create_scalar_function(
        "kodstar_postings",
        2,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let bytes = context.get::<Vec<u8>>(0)?;
            let count = context.get::<usize>(1)?;
            let mut pairs = Vec::new();
            let (mut previous, mut value, mut shift) = (0u64, 0u64, 0);
            for byte in bytes {
                if shift > 63 || shift == 63 && byte > 1 {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                value |= u64::from(byte & 127) << shift;
                if byte & 128 != 0 {
                    shift += 7;
                    continue;
                }
                previous += value >> 1;
                pairs.push([previous, value & 1]);
                (value, shift) = (0, 0);
            }
            if shift != 0 || pairs.len() != count {
                return Err(rusqlite::Error::InvalidQuery);
            }
            serde_json::to_string(&pairs)
                .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
        },
    )
}

pub fn display_path(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "kodstar_display_path",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |context| Ok(String::from_utf8_lossy(context.get_raw(0).as_blob()?).into_owned()),
    )
}
