// Purpose: 원문 청크 형식 1·2·3을 검증하며 제한 인코딩 형식으로 전환.
// Pattern: Streaming Migration.
// Usage: 명시적 업그레이드의 transaction 안에서만 실행.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/mod.rs, crates/kodstar-core/src/infrastructure/history/content/chunks.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::Connection;

#[cfg(test)]
mod tests;

pub(super) fn convert(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    cancel.check()?;
    let version = super::known_schema(connection)?;
    crate::infrastructure::history::maintenance::verification::references(connection)?;
    if version >= 2 {
        let mut select = connection
            .prepare("SELECT hash,content,lines,first_newline,last_newline FROM chunk_data")?;
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let content = row.get_ref(1)?.as_blob().map_err(|_| invalid())?;
            let hash = row.get_ref(0)?.as_blob().map_err(|_| invalid())?;
            if content.is_empty()
                || content.len() > 65536
                || blake3::hash(content).as_bytes().as_slice() != hash
                || crate::infrastructure::history::content::chunks::line_info(content)
                    != (row.get(2)?, row.get(3)?, row.get(4)?)
            {
                return Err(invalid());
            }
        }
        connection.execute_batch(
            "DROP VIEW chunks; ALTER TABLE chunk_refs RENAME TO legacy_chunk_refs;
            ALTER TABLE chunk_data RENAME TO legacy_chunk_data;",
        )?;
    } else {
        connection.execute_batch("ALTER TABLE chunks RENAME TO legacy_chunks;")?;
    }
    connection.execute_batch(crate::infrastructure::history::content::chunks::SCHEMA)?;
    let mut select = connection.prepare("SELECT id,hash,size FROM blobs ORDER BY id")?;
    let mut blobs = select.query([])?;
    let sql = if version == 1 {
        "SELECT part,content FROM legacy_chunks WHERE blob=?1 ORDER BY part"
    } else {
        "SELECT r.part,d.content FROM legacy_chunk_refs r JOIN legacy_chunk_data d ON d.hash=r.hash WHERE r.blob=?1 ORDER BY r.part"
    };
    let mut chunks = connection.prepare(sql)?;
    let mut writer = crate::infrastructure::history::content::chunks::Writer::new(connection)?;
    while let Some(row) = blobs.next()? {
        cancel.check()?;
        let id: i64 = row.get(0)?;
        let expected: String = row.get(1)?;
        let expected_size: u64 = row.get(2)?;
        let mut rows = chunks.query([id])?;
        let mut hash = blake3::Hasher::new();
        let (mut size, mut old_part, mut part, mut used) = (0u64, 0u64, 0u64, 0usize);
        let mut buffer = [0; 65536];
        while let Some(row) = rows.next()? {
            cancel.check()?;
            if row.get::<_, u64>(0)? != old_part {
                return Err(invalid());
            }
            old_part += 1;
            let mut bytes = row.get_ref(1)?.as_blob().map_err(|_| invalid())?;
            size = size.checked_add(bytes.len() as u64).ok_or_else(invalid)?;
            if size > expected_size || bytes.is_empty() || bytes.len() > 65536 {
                return Err(invalid());
            }
            hash.update(bytes);
            while !bytes.is_empty() {
                let take = bytes.len().min(buffer.len() - used);
                buffer[used..used + take].copy_from_slice(&bytes[..take]);
                used += take;
                bytes = &bytes[take..];
                if used == buffer.len() {
                    writer.put(id, part, part * 65536, &buffer, None)?;
                    part += 1;
                    used = 0;
                }
            }
        }
        if used > 0 {
            writer.put(id, part, part * 65536, &buffer[..used], None)?;
        }
        if size != expected_size || hash.finalize().to_hex().as_str() != expected {
            return Err(invalid());
        }
    }
    if version >= 2 {
        // An explicit format change preserves known content even without a current blob ref.
        let mut statement = connection.prepare(
            "SELECT d.content FROM legacy_chunk_data d
            WHERE NOT EXISTS(SELECT 1 FROM chunk_data n WHERE n.hash=d.hash)",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let bytes = row.get_ref(0)?.as_blob().map_err(|_| invalid())?;
            writer.store(bytes, None)?;
        }
    }
    drop(writer);
    drop(blobs);
    drop(select);
    drop(chunks);
    connection.execute_batch(if version == 1 {
        "DROP TABLE legacy_chunks;"
    } else {
        "DROP TABLE legacy_chunk_refs; DROP TABLE legacy_chunk_data;"
    })?;
    crate::infrastructure::history::maintenance::verification::references(connection)
}

fn invalid() -> Error {
    Error::operation("history content checksum mismatch; format was preserved")
}
