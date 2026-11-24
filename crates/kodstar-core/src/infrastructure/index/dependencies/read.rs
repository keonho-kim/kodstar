// Purpose: 선택한 lookup key의 소비 사실을 JSON 변환 없이 직접 수집.
// Pattern: Streaming Reader.
// Usage: invalidation이 합친 pending_lookup_keys를 기존 압축 의존성과 연결.
// Related: crates/kodstar-core/src/infrastructure/index/invalidation.rs, crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::Connection;

pub(crate) fn affect(c: &Connection) -> Result<()> {
    consumers(c, false)
}

pub(crate) fn affect_imports(c: &Connection) -> Result<()> {
    consumers(c, true)
}

fn consumers(c: &Connection, imports: bool) -> Result<()> {
    let mut blocks = c.prepare(
        "SELECT b.count,b.payload FROM pending_lookup_keys k
        CROSS JOIN lookup_rows b ON b.key_id=k.id
        WHERE k.id NOT IN (SELECT id FROM invalidated_lookup_keys)",
    )?;
    let mut rows = blocks.query([])?;
    let mut insert = c.prepare(if imports {
        "INSERT OR IGNORE INTO affected_facts SELECT id FROM facts_rows WHERE id=?1 AND relation IN
         (SELECT id FROM atoms WHERE value IN ('imports','reexports','includes','depends_on','template_include','template_extends'))"
    } else { "INSERT OR IGNORE INTO affected_facts VALUES(?1)" })?;
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
    if !imports {
        c.execute_batch(
            "INSERT OR IGNORE INTO invalidated_lookup_keys SELECT id FROM pending_lookup_keys;",
        )?;
    }
    c.execute("DELETE FROM pending_lookup_keys", [])?;
    Ok(())
}
