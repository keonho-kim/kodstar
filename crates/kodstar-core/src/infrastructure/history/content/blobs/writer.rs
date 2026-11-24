// Purpose: 원문 한 번의 순회로 blob 해시와 청크를 확정.
// Pattern: Streaming Storage Writer.
// Usage: 수집 transaction이 발견 시점의 파일 크기와 열린 입력을 전달.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs, crates/kodstar-core/src/infrastructure/history/content/chunks.rs.

use super::CHUNK;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::io::Read;

pub(crate) struct Writer<'conn> {
    connection: &'conn Connection,
    chunks: super::super::chunks::Writer<'conn>,
    staging: Option<Connection>,
}

impl<'conn> Writer<'conn> {
    pub(crate) fn new(connection: &'conn Connection) -> Result<Self> {
        Ok(Self {
            connection,
            chunks: super::super::chunks::Writer::new(connection)?,
            staging: None,
        })
    }

    pub(crate) fn put(
        &mut self,
        mut input: impl Read,
        size: u64,
        previous: Option<i64>,
        cancel: &Cancellation,
    ) -> Result<i64> {
        if size > CHUNK as u64 {
            return self.put_large(input, size, cancel);
        }
        let mut bytes = [0; CHUNK];
        let read = super::super::chunks::read(&mut input, &mut bytes[..size as usize], cancel)?;
        if read as u64 != size || super::super::chunks::read(&mut input, &mut [0], cancel)? != 0 {
            return Err(changed_input());
        }
        let bytes = &bytes[..read];
        let hash = blake3::hash(bytes);
        if let Some(id) = existing(self.connection, &hash, size)? {
            return Ok(id);
        }
        let base: Option<i64> = self.connection.query_row(
            "SELECT CASE WHEN d.codec=2 THEN d.base ELSE d.id END
             FROM chunk_refs r JOIN chunk_data d ON d.id=r.chunk WHERE r.blob=?1 AND r.part=0
             AND NOT EXISTS(SELECT 1 FROM chunk_refs other WHERE other.blob=r.blob AND other.part>0)",
            [previous], |r| r.get(0),
        ).optional()?.flatten();
        let id = create(self.connection, &hash, size)?;
        if !bytes.is_empty() {
            let chunk = self.chunks.store_hashed(bytes, base, &hash)?;
            self.chunks.reference(id, 0, 0, chunk)?;
        }
        Ok(id)
    }

    fn put_large(&mut self, input: impl Read, size: u64, cancel: &Cancellation) -> Result<i64> {
        if self.staging.is_none() {
            let staging = Connection::open("")?;
            staging.execute_batch(
                "PRAGMA cache_size=-128;
                 CREATE TABLE pending_chunks(part INTEGER PRIMARY KEY,start INTEGER,chunk INTEGER);",
            )?;
            self.staging = Some(staging);
        }
        let staging = self
            .staging
            .as_mut()
            .ok_or_else(|| Error::operation("missing chunk staging"))?;
        // Staging lives on its own temporary connection so even a duplicate blob
        // leaves the history connection's persistent pages and write count untouched.
        let pending = staging.transaction()?;
        let mut insert = pending.prepare("INSERT INTO pending_chunks VALUES(?1,?2,?3)")?;
        let connection = self.connection;
        let before = connection.total_changes();
        connection.execute_batch("SAVEPOINT incoming_blob;")?;
        let mut duplicate = false;
        let result = (|| {
            let mut hash = blake3::Hasher::new();
            let mut start = 0u64;
            let mut part = 0u64;
            super::super::chunking::visit(input, cancel, |bytes| {
                let end = start
                    .checked_add(bytes.len() as u64)
                    .filter(|end| *end <= size)
                    .ok_or_else(changed_input)?;
                hash.update(bytes);
                let chunk = self.chunks.store(bytes, None)?;
                insert.execute(params![part, start, chunk])?;
                start = end;
                part += 1;
                Ok(())
            })?;
            if start != size {
                return Err(changed_input());
            }
            let hash = hash.finalize();
            if let Some(id) = existing(connection, &hash, size)? {
                duplicate = true;
                return Ok(id);
            }
            let id = create(connection, &hash, size)?;
            let mut read =
                pending.prepare("SELECT part,start,chunk FROM pending_chunks ORDER BY part")?;
            let mut rows = read.query([])?;
            while let Some(row) = rows.next()? {
                cancel.check()?;
                self.chunks
                    .reference(id, row.get(0)?, row.get(1)?, row.get(2)?)?;
            }
            Ok(id)
        })();
        let wrote_chunks = connection.total_changes() != before;
        if result.is_err() || duplicate && wrote_chunks {
            connection.execute_batch("ROLLBACK TO incoming_blob; RELEASE incoming_blob;")?;
            if wrote_chunks {
                self.chunks = super::super::chunks::Writer::new(connection)?;
            }
        } else {
            connection.execute_batch("RELEASE incoming_blob;")?;
        }
        result
    }
}

fn existing(connection: &Connection, hash: &blake3::Hash, size: u64) -> Result<Option<i64>> {
    let previous: Option<(i64, u64)> = connection
        .prepare_cached("SELECT id,size FROM blobs WHERE hash=?1")?
        .query_row([hash.as_bytes().as_slice()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    match previous {
        Some((_, stored)) if stored != size => {
            Err(Error::operation("stored history content size mismatch"))
        }
        value => Ok(value.map(|(id, _)| id)),
    }
}

fn create(connection: &Connection, hash: &blake3::Hash, size: u64) -> Result<i64> {
    connection
        .prepare_cached("INSERT INTO blobs(hash,size) VALUES(?1,?2)")?
        .execute(params![hash.as_bytes().as_slice(), size])?;
    Ok(connection.last_insert_rowid())
}

fn changed_input() -> Error {
    Error::operation("history input changed during capture")
}
