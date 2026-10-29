// Purpose: 기존 경로·해시·계보를 보존하며 이력 메타데이터를 정규화.
// Pattern: Transactional Migration.
// Usage: history upgrade의 단일 transaction과 doctor가 호출.
// Related: crates/kodstar-core/src/infrastructure/history/metadata.sql, crates/kodstar-core/src/infrastructure/history/maintenance/metadata/ancestry.rs.

mod ancestry;
#[cfg(test)]
mod tests;
mod verification;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use crate::infrastructure::{cache::paths::path_from_bytes, history::paths};
use ancestry::Ancestry;
use rusqlite::{Connection, params};
pub(super) use verification::run as verify;

pub(super) fn convert(c: &Connection, cancel: &Cancellation) -> Result<()> {
    digests(c, cancel)?;
    c.execute_batch("ALTER TABLE entries RENAME TO metadata_old_entries;")?;
    c.execute_batch(include_str!("../../metadata.sql"))?;
    let mut ancestry = Ancestry::new(c, cancel)?;
    let mut select=c.prepare("SELECT e.checkpoint,e.path,e.blob,e.mode,e.kind,c.parent FROM metadata_old_entries e JOIN checkpoints c ON c.id=e.checkpoint ORDER BY e.checkpoint,e.path")?;
    let mut rows = select.query([])?;
    let mut previous=c.prepare("SELECT checkpoint,blob,mode,kind FROM metadata_old_entries WHERE path=?1 AND checkpoint<=?2 ORDER BY checkpoint DESC")?;
    let mut insert_path = c.prepare("INSERT OR IGNORE INTO paths(value) VALUES(?1)")?;
    let mut find_path = c.prepare("SELECT id FROM paths WHERE value=?1")?;
    let mut insert = c.prepare("INSERT INTO entry_rows VALUES(?1,?2,?3,?4,?5,?6)")?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let checkpoint: i64 = row.get(0)?;
        let path: Vec<u8> = row.get(1)?;
        paths::relative(&path_from_bytes(&path)?)?;
        let mut before = None;
        if let Some(parent) = row.get::<_, Option<i64>>(5)? {
            let mut versions = previous.query(params![path, parent])?;
            while let Some(version) = versions.next()? {
                cancel.check()?;
                if ancestry.contains(parent, version.get(0)?)? {
                    if let Some(blob) = version.get::<_, Option<i64>>(1)? {
                        before = Some((
                            blob,
                            attributes(version.get(2)?, &version.get::<_, String>(3)?)?,
                        ));
                    }
                    break;
                }
            }
        }
        insert_path.execute([&path])?;
        let path: i64 = find_path.query_row([&path], |r| r.get(0))?;
        insert.execute(params![
            checkpoint,
            path,
            row.get::<_, Option<i64>>(2)?,
            attributes(row.get(3)?, &row.get::<_, String>(4)?)?,
            before.map(|b| b.0),
            before.map(|b| b.1)
        ])?;
    }
    drop(rows);
    drop(select);
    drop(previous);
    drop(insert_path);
    drop(find_path);
    drop(insert);
    drop(ancestry);
    c.execute_batch("DROP TABLE metadata_old_entries; DROP TABLE metadata_ancestry;")?;
    Ok(())
}

pub(super) fn attributes(mode: u32, kind: &str) -> Result<i64> {
    if mode > 511 || !matches!(kind, "file" | "symlink") {
        return Err(Error::operation("invalid history file attributes"));
    }
    Ok(i64::from(mode) | if kind == "symlink" { 512 } else { 0 })
}

fn digests(c: &Connection, cancel: &Cancellation) -> Result<()> {
    c.execute_batch("CREATE TABLE metadata_blobs(id INTEGER PRIMARY KEY,hash BLOB NOT NULL UNIQUE CHECK(length(hash)=32),size INTEGER NOT NULL DEFAULT 0);")?;
    let mut select = c.prepare("SELECT id,hash,size FROM blobs ORDER BY id")?;
    let mut rows = select.query([])?;
    let mut insert = c.prepare("INSERT INTO metadata_blobs VALUES(?1,?2,?3)")?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let hash: String = row.get(1)?;
        let hash = blake3::Hash::from_hex(&hash)
            .map_err(|e| Error::caused_by("invalid legacy history hash", e))?;
        insert.execute(params![
            row.get::<_, i64>(0)?,
            hash.as_bytes().as_slice(),
            row.get::<_, u64>(2)?
        ])?;
    }
    drop(rows);
    drop(select);
    drop(insert);
    c.execute_batch("DROP TABLE blobs; ALTER TABLE metadata_blobs RENAME TO blobs;")?;
    Ok(())
}
