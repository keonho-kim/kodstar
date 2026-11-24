// Purpose: 탐색 정렬에 필요한 정점 위치·이름·경로만 ID로 정확 조회.
// Pattern: Storage Projection.
// Usage: query/traversal이 snapshot의 임시 view를 준비하고 이웃·대기열 정렬에 사용.
// Related: crates/kodstar-core/src/infrastructure/index/records/symbols.sql, crates/kodstar-core/src/application/query/traversal/disk.rs.

use rusqlite::{Connection, Result};

pub(crate) fn prepare(connection: &Connection) -> Result<()> {
    // A multi-table view on the right of LEFT JOIN is materialized by SQLite.
    // A single-table projection keeps both scalar lookups tied to this one node ID.
    connection.execute_batch(
        "CREATE TEMP VIEW IF NOT EXISTS query_nodes AS
        SELECT s.id,s.file_id,s.line,s.column_byte,
            (SELECT value FROM atoms WHERE id=s.qualified_id) AS qualified_name,
            (SELECT path FROM files_rows WHERE id=s.file_id) AS path
        FROM symbols_rows s;",
    )
}
