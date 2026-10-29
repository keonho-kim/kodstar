// Purpose: 정규 경로와 변경 전 상태를 원래 변경 계보와 독립 대조.
// Pattern: Integrity Check.
// Usage: 명시 전환과 doctor가 모든 보존 변경 행을 스트리밍 검증.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/metadata/mod.rs.

use super::{Ancestry, attributes};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::{cache::paths::path_from_bytes, history::paths};
use rusqlite::{Connection, params};

pub(in crate::infrastructure::history::maintenance) fn run(
    c: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    let mut paths = c.prepare("SELECT value FROM paths")?;
    let mut paths = paths.query([])?;
    while let Some(row) = paths.next()? {
        cancel.check()?;
        paths::relative(&path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?)?;
    }
    let mut ancestry = Ancestry::new(c, cancel)?;
    let mut select=c.prepare("SELECT e.path,c.parent,e.before_blob,e.before_attributes FROM entry_rows e JOIN checkpoints c ON c.id=e.checkpoint")?;
    let mut rows = select.query([])?;
    let mut previous=c.prepare("SELECT checkpoint,blob,attributes FROM entry_rows WHERE path=?1 AND checkpoint<=?2 ORDER BY checkpoint DESC")?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let mut expected = None;
        if let Some(parent) = row.get::<_, Option<i64>>(1)? {
            let mut versions = previous.query(params![row.get::<_, i64>(0)?, parent])?;
            while let Some(version) = versions.next()? {
                cancel.check()?;
                if ancestry.contains(parent, version.get(0)?)? {
                    if let Some(blob) = version.get::<_, Option<i64>>(1)? {
                        expected = Some((blob, version.get::<_, i64>(2)?));
                    }
                    break;
                }
            }
        }
        let actual = (row.get::<_, Option<i64>>(2)?, row.get::<_, Option<i64>>(3)?);
        if actual != (expected.map(|v| v.0), expected.map(|v| v.1)) {
            return Err(Error::operation(
                "history previous file state does not match its ancestry",
            ));
        }
    }
    drop(rows);
    drop(select);
    drop(previous);
    drop(ancestry);
    c.execute_batch("DROP TABLE metadata_ancestry;")?;
    let mut working = c.prepare("SELECT path,mode,kind FROM working")?;
    let mut rows = working.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        paths::relative(&path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?)?;
        attributes(row.get(1)?, &row.get::<_, String>(2)?)?;
    }
    Ok(())
}
