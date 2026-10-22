// Purpose: 큰 단일 key를 경계 문자열 중복 없이 저장·정확 검색.
// Pattern: Bounded Overflow Block.
// Usage: dictionary writer와 cursor가 64KiB 넘는 원래 key를 보존.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs, crates/kodstar-core/src/infrastructure/index/dictionary/publication.rs.

use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

pub(super) fn candidate(
    c: &Connection,
    kind: &Value,
    key: &Value,
    after: i64,
) -> Result<Option<i64>> {
    if let Value::Text(key) = key {
        let digest = blake3::hash(key.as_bytes());
        c.query_row("SELECT id FROM dictionary_blocks WHERE kind=?1 AND digest=?2 AND id>?3 ORDER BY id LIMIT 1",params![kind,digest.as_bytes().as_slice(),after],|r|r.get(0)).optional()
    } else {
        // Let SQLite apply text affinity when a non-text SQL value was supplied.
        c.query_row("SELECT id FROM dictionary_blocks WHERE kind=?1 AND digest IS NOT NULL AND id>?2 ORDER BY id LIMIT 1",params![kind,after],|r|r.get(0)).optional()
    }
}

pub(super) fn intern(c: &Connection, kind: &str, key: &str, next: &mut i64) -> Result<i64> {
    let digest = blake3::hash(key.as_bytes());
    let mut select = c.prepare(
        "SELECT payload,count FROM dictionary_blocks WHERE kind=?1 AND digest=?2 ORDER BY id",
    )?;
    let mut rows = select.query(params![kind, digest.as_bytes().as_slice()])?;
    while let Some(row) = rows.next()? {
        let bytes = row
            .get_ref(0)?
            .as_blob()
            .map_err(|_| super::super::packed::codec::invalid())?;
        let entries = super::codec::decode(bytes, row.get(1)?)?;
        if entries.len() != 1 {
            return Err(super::super::packed::codec::invalid());
        }
        if entries[0].1 == key {
            return Ok(entries[0].0);
        }
    }
    drop(rows);
    drop(select);
    *next = next
        .checked_add(1)
        .ok_or_else(super::super::packed::codec::invalid)?;
    super::store(c, 0, kind, &[(*next, key.to_owned())])?;
    Ok(*next)
}
