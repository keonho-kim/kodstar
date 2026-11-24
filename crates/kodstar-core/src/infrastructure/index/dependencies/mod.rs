// Purpose: 조회 의존성을 파일·key별 제한된 차이 정수 블록으로 저장.
// Pattern: Storage Adapter.
// Usage: resolver publication과 index 연결 초기화가 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs, crates/kodstar-core/src/infrastructure/index/dependencies/schema.sql.

mod codec;
pub(in crate::infrastructure::index) mod pages;
pub(crate) use pages::{clear_all, clear_file};
mod publication;
mod read;
mod selection;
mod writer;
pub(crate) use publication::publish;
pub(crate) use read::affect;
pub(crate) use read::affect_imports;
pub(crate) use selection::prepare_selected;
#[cfg(test)]
mod tests;

use crate::domain::diagnostics::Result;
use rusqlite::Connection;

pub(crate) fn register(connection: &Connection) -> rusqlite::Result<()> {
    super::packed::expansion::register(connection, c"kodstar_posting_rows", |bytes, count| {
        let mut rows = Vec::new();
        codec::visit(
            bytes,
            usize::try_from(count).map_err(|_| rusqlite::Error::InvalidQuery)?,
            |fact, found| {
                rows.push([fact, i64::from(found)]);
                Ok(())
            },
        )
        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
        Ok(rows)
    })
}

pub(crate) fn verify(connection: &Connection) -> Result<()> {
    let invalid: bool = connection.query_row(
        "SELECT EXISTS(
        SELECT 1 FROM lookup_blocks b,kodstar_posting_rows(b.payload,b.count) j
        LEFT JOIN facts f ON f.id=j.first
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
