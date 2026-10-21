// Purpose: 정상 세대의 coverage를 원자적으로 저장하고 상수 비용으로 조회.
// Pattern: Storage Adapter.
// Usage: analysis는 commit 전에 refresh하고 읽기 snapshot은 read를 호출.
// Related: crates/kodstar-core/src/infrastructure/index/schema.sql, crates/kodstar-core/src/application/analysis/mod.rs.

use crate::domain::diagnostics::{Coverage, Result};
use rusqlite::Connection;

pub(super) fn refresh(connection: &Connection, generation: i64) -> Result<()> {
    connection.execute(
        "INSERT OR REPLACE INTO coverage
         SELECT 1,?1,COUNT(*),COALESCE(SUM(status='analyzed'),0),
           COALESCE(SUM(status='skipped'),0),COALESCE(SUM(status='unsupported'),0),
           COALESCE(SUM(CASE WHEN status='analyzed' THEN size ELSE 0 END),0),
           (SELECT COUNT(DISTINCT logical_key) FROM symbols),
           (SELECT COUNT(*) FROM relationships WHERE resolution='resolved'),
           (SELECT COUNT(DISTINCT fact_id) FROM relationships
             WHERE resolution IN ('unresolved','ambiguous'))
         FROM files",
        [generation],
    )?;
    Ok(())
}

pub(super) fn read(connection: &Connection) -> Result<Coverage> {
    Ok(connection.query_row(
        "SELECT c.scanned_files,c.analyzed_files,c.skipped_files,c.unsupported_files,
                c.source_bytes,c.symbols,c.relationships,c.unresolved
         FROM coverage c JOIN metadata m ON m.generation=c.generation
         WHERE c.singleton=1 AND m.singleton=1",
        [],
        |row| {
            Ok(Coverage {
                scanned_files: row.get(0)?,
                analyzed_files: row.get(1)?,
                skipped_files: row.get(2)?,
                unsupported_files: row.get(3)?,
                source_bytes: row.get(4)?,
                symbols: row.get(5)?,
                relationships: row.get(6)?,
                unresolved: row.get(7)?,
                reused_files: 0,
            })
        },
    )?)
}
