// Purpose: 바이트 한도가 고정된 소스 청크의 저장·복원과 중복 제거.
// Pattern: Storage Adapter.
// Usage: capture·checkout·analysis 입력이 blob을 스트리밍.
// Related: crates/kodstar-core/src/infrastructure/history/schema.sql.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::io::{Read, Seek, SeekFrom, Write};

pub(crate) const CHUNK: usize = 64 * 1024;

pub(crate) fn put(
    connection: &Connection,
    mut input: impl Read + Seek,
    previous_blob: Option<i64>,
    cancel: &Cancellation,
) -> Result<i64> {
    let start = input
        .stream_position()
        .map_err(|e| Error::caused_by("locate history input", e))?;
    let mut buffer = [0u8; CHUNK];
    let (expected, expected_size) = fingerprint(&mut input, &mut buffer, cancel)?;
    let previous: Option<(i64, u64)> = connection
        .query_row(
            "SELECT id,size FROM blobs WHERE hash=?1",
            [&expected],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, size)) = previous {
        if size != expected_size {
            return Err(Error::operation("stored history content size mismatch"));
        }
        cancel.check()?;
        return Ok(id);
    }
    input
        .seek(SeekFrom::Start(start))
        .map_err(|e| Error::caused_by("rewind history input", e))?;
    cancel.check()?;
    // The capture transaction owns these writes and rolls back an unstable reread.
    connection.execute(
        "INSERT INTO blobs(hash,size) VALUES(?1,?2)",
        params![expected, expected_size],
    )?;
    let id = connection.last_insert_rowid();
    let mut hash = blake3::Hasher::new();
    let mut size = 0u64;
    let mut part = 0u64;
    let base: Option<i64> = if expected_size <= CHUNK as u64 {
        connection.query_row("SELECT CASE WHEN d.codec=2 THEN d.base ELSE d.id END
            FROM chunk_refs r JOIN chunk_data d ON d.id=r.chunk WHERE r.blob=?1 AND r.part=0
            AND NOT EXISTS(SELECT 1 FROM chunk_refs other WHERE other.blob=r.blob AND other.part>0)",
            [previous_blob], |r| r.get(0)).optional()?.flatten()
    } else {
        None
    };
    let mut input = std::io::BufReader::with_capacity(CHUNK, input);
    let mut insert = super::chunks::Writer::new(connection)?;
    loop {
        cancel.check()?;
        let read = if expected_size <= CHUNK as u64 {
            super::chunks::read(&mut input, &mut buffer, cancel)?
        } else {
            super::chunking::read(&mut input, &mut buffer, cancel)?
        };
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| Error::operation("history input size overflow"))?;
        if size > expected_size {
            return Err(Error::operation("history input changed during capture"));
        }
        hash.update(&buffer[..read]);
        insert.put(id, part, size - read as u64, &buffer[..read], base)?;
        part += 1;
    }
    if hash.finalize().to_hex().as_str() != expected || size != expected_size {
        return Err(Error::operation("history input changed during capture"));
    }
    cancel.check()?;
    Ok(id)
}

fn fingerprint(
    input: &mut impl Read,
    buffer: &mut [u8; CHUNK],
    cancel: &Cancellation,
) -> Result<(String, u64)> {
    let mut hash = blake3::Hasher::new();
    let mut size = 0u64;
    loop {
        cancel.check()?;
        let read = input
            .read(buffer)
            .map_err(|e| Error::caused_by("hash history input", e))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| Error::operation("history input size overflow"))?;
    }
    Ok((hash.finalize().to_hex().to_string(), size))
}

pub(crate) fn write(
    connection: &Connection,
    id: i64,
    mut output: impl Write,
    cancel: &Cancellation,
) -> Result<()> {
    let (expected, expected_size): (String, u64) =
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
        let content = super::chunk_read::load(connection, row.get(2)?)?;
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
    if hash.finalize().to_hex().as_str() != expected || size != expected_size {
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
    let size: u64 =
        connection.query_row("SELECT size FROM blobs WHERE id=?1", [id], |r| r.get(0))?;
    if size > max as u64 {
        return Err(Error::operation(
            "stored source exceeds requested input limit",
        ));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    write(connection, id, &mut bytes, cancel)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
