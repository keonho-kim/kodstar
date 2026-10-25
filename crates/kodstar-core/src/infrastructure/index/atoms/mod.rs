// Purpose: 공유 분석 문자열의 저장 수명과 바이너리 식별자 경계 변환.
// Pattern: Storage Adapter.
// Usage: index 연결·records trigger·분석 transaction이 호출.
// Related: crates/kodstar-core/src/infrastructure/index/atoms/schema.sql, crates/kodstar-core/src/infrastructure/index/records/mod.rs.

mod verification;
pub(crate) mod writer;
use rusqlite::{Connection, Result, functions::FunctionFlags, types::ValueRef};
pub(crate) use verification::verify;

pub(crate) const SCHEMA: &str = include_str!("schema.sql");

pub(crate) fn register(c: &Connection) -> Result<()> {
    c.create_scalar_function(
        "kodstar_digest",
        1,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |ctx| match ctx.get_raw(0) {
            ValueRef::Null => Ok(None),
            ValueRef::Blob(bytes) if bytes.len() == 32 => Ok(Some(bytes.to_vec())),
            ValueRef::Text(bytes) => {
                let text = std::str::from_utf8(bytes)
                    .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
                let hash = blake3::Hash::from_hex(text)
                    .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
                Ok(Some(hash.as_bytes().to_vec()))
            }
            _ => Err(rusqlite::Error::InvalidQuery),
        },
    )
}

pub(crate) fn collect(c: &Connection) -> Result<()> {
    c.execute("DELETE FROM atoms WHERE refs=0", [])?;
    Ok(())
}
