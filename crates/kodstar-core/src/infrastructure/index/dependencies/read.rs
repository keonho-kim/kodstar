// Purpose: 선택한 lookup key의 소비 사실을 JSON 변환 없이 직접 수집.
// Pattern: Streaming Reader.
// Usage: invalidation이 합친 pending_lookup_keys를 기존 압축 의존성과 연결.
// Related: crates/kodstar-core/src/infrastructure/index/invalidation.rs, crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::Connection;

pub(crate) fn affect(c: &Connection) -> Result<()> {
    let mut blocks = c.prepare(
        "SELECT b.count,b.payload FROM pending_lookup_keys k
        CROSS JOIN lookup_rows b ON b.key_id=k.id
        WHERE k.id NOT IN (SELECT id FROM invalidated_lookup_keys)",
    )?;
    let mut rows = blocks.query([])?;
    let mut insert = c.prepare("INSERT OR IGNORE INTO affected_facts VALUES(?1)")?;
    while let Some(row) = rows.next()? {
        let bytes = row
            .get_ref(1)?
            .as_blob()
            .map_err(|e| Error::caused_by("lookup payload", e))?;
        super::codec::visit(bytes, row.get(0)?, |fact, _| {
            insert.execute([fact])?;
            Ok(())
        })?;
    }
    c.execute_batch(
        "INSERT OR IGNORE INTO invalidated_lookup_keys SELECT id FROM pending_lookup_keys;
        DELETE FROM pending_lookup_keys;",
    )?;
    Ok(())
}
