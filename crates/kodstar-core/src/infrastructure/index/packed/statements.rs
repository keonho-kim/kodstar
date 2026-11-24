// Purpose: 압축 저장소 cursor 안의 짧은 고정 SQL 준비 비용을 제한된 크기로 재사용.
// Pattern: Bounded Statement Reuse.
// Usage: 관계 주소·블록·역조회·공유 정보 reader가 호출.
// Related: crates/kodstar-core/src/infrastructure/index/packed/mod.rs, crates/kodstar-core/src/infrastructure/index/relations/mod.rs.

use rusqlite::{Connection, Params, Result, Row, StatementStatus};

pub(in crate::infrastructure::index) fn row<T>(
    connection: &Connection,
    sql: &str,
    parameters: impl Params,
    read: impl FnOnce(&Row<'_>) -> Result<T>,
) -> Result<T> {
    let mut statement = connection.prepare_cached(sql)?;
    let result = statement.query_row(parameters, read);
    let limit = (connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize / 64)
        .min(16 * 1024);
    if statement.get_status(StatementStatus::MemUsed) as usize > limit {
        statement.discard();
    }
    // CachedStatement releases bindings before returning the statement to the connection.
    result
}

pub(in crate::infrastructure::index) fn execute(
    connection: &Connection,
    sql: &str,
    parameters: impl Params,
) -> Result<usize> {
    let mut statement = connection.prepare_cached(sql)?;
    let result = statement.execute(parameters);
    let limit = (connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize / 64)
        .min(16 * 1024);
    if statement.get_status(StatementStatus::MemUsed) as usize > limit {
        statement.discard();
    }
    result
}
