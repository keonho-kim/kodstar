// Purpose: 영구 이력의 참조·내용 무결성 검증.
// Pattern: Integrity Check.
// Usage: doctor가 저장소 잠금 아래 호출.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs.

use super::super::Repository;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};

impl Repository {
    pub fn verify(&self, cancel: &Cancellation) -> Result<()> {
        verify(&self.connection, cancel)
    }
}

pub(super) fn verify(connection: &rusqlite::Connection, cancel: &Cancellation) -> Result<()> {
    references(connection)?;
    let mut chunks = connection.prepare("SELECT id FROM chunk_data")?;
    let mut chunks = chunks.query([])?;
    while let Some(row) = chunks.next()? {
        cancel.check()?;
        super::super::content::chunk_read::load(connection, row.get(0)?)?;
    }
    let mut statement = connection.prepare("SELECT id FROM blobs ORDER BY id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        super::super::content::blobs::write(connection, row.get(0)?, std::io::sink(), cancel)?;
    }
    Ok(())
}

pub(super) fn references(connection: &rusqlite::Connection) -> Result<()> {
    let result: String = connection.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if result != "ok" {
        return Err(Error::operation("history database integrity check failed"));
    }
    let mut foreign = connection.prepare("PRAGMA foreign_key_check")?;
    if foreign.query([])?.next()?.is_some() {
        return Err(Error::operation("history foreign-key check failed"));
    }
    let invalid:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM checkpoints WHERE parent>=id) OR NOT EXISTS(SELECT 1 FROM branches b JOIN repository r ON b.name=r.current_branch)",[],|r|r.get(0))?;
    if invalid {
        return Err(Error::operation(
            "history ancestry or current branch is invalid",
        ));
    }
    let invalid_journal: bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM checkout_files f WHERE (before_blob IS NOT NULL AND NOT EXISTS(SELECT 1 FROM blobs b WHERE b.id=f.before_blob)) OR (after_blob IS NOT NULL AND NOT EXISTS(SELECT 1 FROM blobs b WHERE b.id=f.after_blob)))",[],|r|r.get(0))?;
    if invalid_journal {
        return Err(Error::operation("history checkout references are invalid"));
    }
    Ok(())
}
