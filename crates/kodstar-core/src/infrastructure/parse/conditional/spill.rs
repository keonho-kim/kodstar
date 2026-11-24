// Purpose: 큰 조건부 병합의 정규 key를 제한된 SQLite 임시 저장소에서 정확히 대조.
// Pattern: Spill Adapter.
// Usage: in-memory reducer의 예산이 차면 전환하며 connection 종료 시 SQLite가 파일을 제거.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/facts.rs, crates/kodstar-core/src/infrastructure/parse/conditional/spill/stream.rs.

mod stream;
#[cfg(test)]
mod tests;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

pub(super) struct Spill {
    connection: Connection,
}

impl Spill {
    pub(super) fn new(memory_budget: usize) -> Result<Self> {
        let connection = Connection::open("")?;
        connection.set_prepared_statement_cache_capacity(4);
        connection.execute_batch("PRAGMA temp_store=FILE; PRAGMA cache_size=-64; PRAGMA mmap_size=0; PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
            CREATE TABLE keys(id INTEGER PRIMARY KEY,kind INTEGER NOT NULL,digest BLOB NOT NULL,bytes INTEGER NOT NULL,value INTEGER NOT NULL);
            CREATE INDEX key_digest ON keys(kind,digest,bytes);
            CREATE TABLE chunks(key INTEGER NOT NULL,part INTEGER NOT NULL,content BLOB NOT NULL,PRIMARY KEY(key,part)) WITHOUT ROWID;
            BEGIN;")?;
        let pages = memory_budget
            .saturating_mul(32)
            .div_ceil(4096)
            .clamp(32, i32::MAX as usize);
        connection.pragma_update(None, "max_page_count", pages)?;
        Ok(Self { connection })
    }

    pub(super) fn find<T: Serialize>(
        &self,
        kind: u8,
        key: &T,
        cancel: &Cancellation,
    ) -> Result<Option<u32>> {
        let (digest, bytes) = stream::digest(key, cancel)?;
        let mut statement = self
            .connection
            .prepare_cached("SELECT id,value FROM keys WHERE kind=?1 AND digest=?2 AND bytes=?3")?;
        let mut rows = statement.query(params![kind, digest.as_bytes().as_slice(), bytes])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let id = row.get(0)?;
            if stream::matches(&self.connection, id, key, cancel)? {
                return Ok(Some(row.get(1)?));
            }
        }
        Ok(None)
    }

    pub(super) fn insert<T: Serialize>(
        &self,
        kind: u8,
        key: &T,
        value: u32,
        cancel: &Cancellation,
    ) -> Result<()> {
        let (digest, bytes) = stream::digest(key, cancel)?;
        self.connection
            .prepare_cached("INSERT INTO keys(kind,digest,bytes,value) VALUES(?1,?2,?3,?4)")?
            .execute(params![kind, digest.as_bytes().as_slice(), bytes, value])
            .map_err(storage_error)?;
        stream::store(
            &self.connection,
            self.connection.last_insert_rowid(),
            key,
            cancel,
        )
    }
}

fn storage_error(error: rusqlite::Error) -> Error {
    let full = matches!(&error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DiskFull);
    let mut result = Error::caused_by("conditional fact spill failed", error);
    if full {
        result.kind = ErrorKind::Limit;
    }
    result
}
