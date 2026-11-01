// Purpose: 조회 의존성을 파일·key별 제한된 차이 정수 블록으로 저장.
// Pattern: Storage Adapter.
// Usage: resolver publication과 index 연결 초기화가 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs, crates/kodstar-core/src/infrastructure/index/dependencies/schema.sql.

mod codec;
mod publication;
mod read;
mod selection;
mod writer;
pub(crate) use publication::publish;
pub(crate) use read::affect;
pub(crate) use selection::prepare_selected;
#[cfg(test)]
mod tests;

use crate::domain::diagnostics::Result;
use rusqlite::{Connection, functions::FunctionFlags};

pub(crate) fn register(connection: &Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "kodstar_postings",
        2,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |context| {
            let bytes = context.get_raw(0).as_blob()?;
            let count = context.get::<usize>(1)?;
            codec::json(bytes, count)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
        },
    )
}

pub(crate) fn verify(connection: &Connection) -> Result<()> {
    let invalid: bool = connection.query_row(
        "SELECT EXISTS(
        SELECT 1 FROM lookup_blocks b,json_each(kodstar_postings(b.payload,b.count)) j
        LEFT JOIN facts f ON f.id=json_extract(j.value,'$[0]')
        WHERE f.id IS NULL OR f.file_id<>b.file_id)",
        [],
        |r| r.get(0),
    )?;
    if invalid {
        return Err(crate::domain::diagnostics::Error::operation(
            "lookup dependency references a missing or different source file",
        ));
    }
    let duplicate: bool = connection.query_row(
        "SELECT EXISTS(
        SELECT 1 FROM fact_lookups GROUP BY key_id,fact_id HAVING count(*)<>1)",
        [],
        |r| r.get(0),
    )?;
    if duplicate {
        return Err(crate::domain::diagnostics::Error::operation(
            "duplicate lookup dependency across blocks",
        ));
    }
    Ok(())
}
