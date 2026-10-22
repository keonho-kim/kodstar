// Purpose: 압축 lookup의 독립 기준 decoder를 통합 테스트 SQL에 제공.
// Pattern: Reference Decoder.
// Usage: snapshot과 publication rollback 사례가 연결에 등록.
// Related: scripts/benchmarks/postings.py, crates/kodstar-core/tests/support/snapshot.rs.

pub fn register(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    super::packed::register(connection)?;
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
