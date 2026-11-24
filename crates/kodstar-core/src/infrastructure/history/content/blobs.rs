// Purpose: 바이트 한도가 고정된 소스 청크의 저장·복원과 중복 제거.
// Pattern: Storage Adapter.
// Usage: capture·checkout·analysis 입력이 blob을 스트리밍.
// Related: crates/kodstar-core/src/infrastructure/history/schema.sql.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::Connection;
use std::io::Write;

mod writer;
pub(crate) use writer::Writer;

pub(crate) const CHUNK: usize = 64 * 1024;

pub(crate) fn write(
    connection: &Connection,
    id: i64,
    output: impl Write,
    cancel: &Cancellation,
) -> Result<()> {
    write_in(
        connection,
        id,
        output,
        cancel,
        &mut super::chunk_read::ReadWorkspace::default(),
    )
}

fn write_in(
    connection: &Connection,
    id: i64,
    mut output: impl Write,
    cancel: &Cancellation,
    workspace: &mut super::chunk_read::ReadWorkspace,
) -> Result<()> {
    let (expected, expected_size): (Vec<u8>, u64) =
        connection.query_row("SELECT hash,size FROM blobs WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let mut statement = connection.prepare(
        "SELECT r.part,r.start,r.chunk FROM chunk_refs r WHERE r.blob=?1 ORDER BY r.part",
    )?;
    let mut rows = statement.query([id])?;
    let mut hash = blake3::Hasher::new();
    let mut size = 0u64;
    let mut part = 0u64;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let content = workspace.load(connection, row.get(2)?)?;
        if row.get::<_, u64>(0)? != part
            || row.get::<_, u64>(1)? != size
            || content.len() as u64 > expected_size.saturating_sub(size)
        {
            return Err(Error::operation(
                "history content checksum mismatch: chunk layout or line metadata",
            ));
        }
        size = size
            .checked_add(content.len() as u64)
            .ok_or_else(|| Error::operation("history content size overflow"))?;
        if size > expected_size {
            return Err(Error::operation(
                "history content checksum mismatch: declared size exceeded",
            ));
        }
        hash.update(&content);
        output
            .write_all(&content)
            .map_err(|e| Error::caused_by("write history content", e))?;
        part += 1;
    }
    if hash.finalize().as_bytes().as_slice() != expected || size != expected_size {
        return Err(Error::operation("history content checksum mismatch"));
    }
    Ok(())
}

pub(crate) fn read(
    connection: &Connection,
    id: i64,
    max: usize,
    cancel: &Cancellation,
) -> Result<Vec<u8>> {
    read_in(
        connection,
        id,
        max,
        cancel,
        &mut super::chunk_read::ReadWorkspace::default(),
    )
}

pub(in crate::infrastructure::history) fn read_in(
    connection: &Connection,
    id: i64,
    max: usize,
    cancel: &Cancellation,
    workspace: &mut super::chunk_read::ReadWorkspace,
) -> Result<Vec<u8>> {
    let size: u64 =
        connection.query_row("SELECT size FROM blobs WHERE id=?1", [id], |r| r.get(0))?;
    if size > max as u64 {
        return Err(Error::operation(
            "stored source exceeds requested input limit",
        ));
    }
    // Keep the native adapter's trailing NUL in the same allocation.
    let capacity = usize::try_from(size)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error::operation("stored source size exceeds addressable memory"))?;
    let mut bytes = Vec::with_capacity(capacity);
    write_in(connection, id, &mut bytes, cancel, workspace)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
