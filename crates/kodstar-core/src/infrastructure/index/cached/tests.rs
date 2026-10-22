// Purpose: SQL 재사용의 동시 cursor·매개변수 정리·예산·오류 후 자원 회수 검증.
// Pattern: Unit Test.
// Usage: index statement pool이 독립 SQLite 연결에서 검증.
// Related: crates/kodstar-core/src/infrastructure/index/cached.rs.

use super::*;

#[test]
fn active_queries_are_not_reused_and_named_bindings_are_cleared() -> rusqlite::Result<()> {
    let connection = Connection::open_in_memory()?;
    connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 32768)?;
    let sql = Sql::new(&connection)?;
    let mut first = sql.cached_prepare("SELECT ?1")?;
    let mut rows = first.query([7])?;
    assert_eq!(sql.cached_row("SELECT ?1", [9], |r| r.get::<_, i64>(0))?, 9);
    assert_eq!(
        rows.next()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?
            .get::<_, i64>(0)?,
        7
    );
    drop(rows);
    drop(first);
    let both = &[(":value", &3), (":other", &4)];
    assert_eq!(
        sql.cached_row("SELECT :value,:other", both, |r| Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<i64>>(1)?
        )))?,
        (3, Some(4))
    );
    assert_eq!(
        sql.cached_row("SELECT :value,:other", &[(":value", &5)], |r| Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<i64>>(1)?
        )))?,
        (5, None)
    );
    Ok(())
}

#[test]
fn cache_eviction_and_failed_queries_release_resources_within_the_budget() -> rusqlite::Result<()> {
    let connection = Connection::open_in_memory()?;
    connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 8192)?;
    let sql = Sql::new(&connection)?;
    for value in 0..160 {
        assert_eq!(
            sql.cached_row(&format!("SELECT {value}"), [], |r| r.get::<_, i64>(0))?,
            value
        );
        assert!(sql.bytes.get() <= sql.limit);
        assert!(sql.idle.borrow().len() <= 64);
        assert!(sql.idle_bytes.get() <= (sql.limit / 4).min(1024 * 1024));
    }
    assert!(
        sql.cached_row("SELECT missing", [], |r| r.get::<_, i64>(0))
            .is_err()
    );
    assert!(
        sql.cached_row("SELECT ?1", ["x".repeat(8193)], |r| r.get::<_, String>(0))
            .is_err()
    );
    assert_eq!(
        sql.cached_row("SELECT ?1", [11], |r| r.get::<_, i64>(0))?,
        11
    );
    assert!(sql.bytes.get() <= sql.limit);
    drop(sql);
    connection.execute_batch("CREATE TABLE released(value); DROP TABLE released;")?;
    Ok(())
}
