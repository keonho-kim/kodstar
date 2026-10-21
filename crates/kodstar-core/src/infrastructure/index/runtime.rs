// Purpose: SQLite 작업의 바이트 한도·취소·오류 경계.
// Pattern: Storage Adapter.
// Usage: 분석·조회가 소유 connection과 명시적 취소 값을 전달.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/src/domain/diagnostics.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use rusqlite::Connection;

pub(crate) fn limit_reads(connection: &Connection, memory_budget: usize) -> Result<()> {
    // A relationship may own several row copies. Bound each SQLite value/record independently
    // of graph output-row and visited-node limits.
    let bytes = (memory_budget / 8).min(i32::MAX as usize) as i32;
    connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, bytes)?;
    Ok(())
}

pub(crate) fn observe_cancellation(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    let cancel = cancel.clone();
    connection.progress_handler(10_000, Some(move || cancel.is_cancelled()))?;
    Ok(())
}

pub(crate) fn stop_observing(connection: &Connection) -> Result<()> {
    // Cleanup must remain possible after cancellation, including an already auto-rolled-back write.
    connection.progress_handler(0, None::<fn() -> bool>)?;
    Ok(())
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        let (kind, message) = match &error {
            rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::TooBig => {
                (
                    ErrorKind::Limit,
                    "SQLite row/value exceeds the working-memory read limit",
                )
            }
            rusqlite::Error::SqliteFailure(code, _)
                if code.code == rusqlite::ErrorCode::OperationInterrupted =>
            {
                (ErrorKind::Cancelled, "SQLite operation cancelled")
            }
            _ => (ErrorKind::Operation, "SQLite index operation failed"),
        };
        let mut failure = Self::caused_by(message, error);
        failure.kind = kind;
        failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interruption_preserves_committed_data_and_allows_cleanup() -> Result<()> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "CREATE TABLE items(id INTEGER PRIMARY KEY); INSERT INTO items VALUES(0);",
        )?;
        let cancel = Cancellation::default();
        observe_cancellation(&connection, &cancel)?;
        let transaction = connection.transaction()?;
        cancel.cancel();
        let failure = transaction.execute_batch(
            "WITH RECURSIVE input(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM input WHERE n<1000000) INSERT INTO items SELECT n FROM input;",
        ).expect_err("progress hook must interrupt SQL work");
        let error = Error::from(failure);
        stop_observing(&transaction)?;
        transaction.finish()?;
        assert_eq!(error.kind, ErrorKind::Cancelled);
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))?;
        assert_eq!(count, 1);
        connection.execute("INSERT INTO items VALUES(1)", [])?;
        Ok(())
    }
}
