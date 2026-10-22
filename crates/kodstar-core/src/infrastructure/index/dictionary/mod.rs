// Purpose: 조회 key의 안정 ID·접두어 블록과 원자적 사전 공개.
// Pattern: Storage Adapter.
// Usage: dependency publication이 stage를 전달하고 packed cursor가 조회.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/schema.sql, crates/kodstar-core/src/infrastructure/index/dependencies/mod.rs.

mod codec;
mod large;
mod retirement;
mod verification;
pub(crate) use retirement::run as retire;
pub(crate) use verification::verify;
#[cfg(test)]
mod tests;

use super::packed::{Filter, Row};
use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

mod publication;
pub(crate) use publication::publish;

pub(crate) const SCHEMA: &str = include_str!("schema.sql");

pub(super) fn scan(
    c: &Connection,
    after: i64,
    filters: &[Filter],
) -> Result<Option<(i64, Vec<Row>)>> {
    let find = |column| filters.iter().find(|(k, _)| *k == column).map(|(_, v)| v);
    let block = if let Some(id) = find(0) {
        c.query_row("SELECT b.id,b.kind,b.count,b.payload FROM dictionary_ids i JOIN dictionary_blocks b ON b.id=i.block WHERE i.id=?1 AND b.id>?2", params![id, after], read).optional()?
    } else if let (Some(kind), Some(key)) = (find(1), find(2)) {
        let normal=c.query_row("SELECT id,kind,count,payload FROM dictionary_blocks WHERE id=(SELECT id FROM dictionary_blocks WHERE kind=?1 AND first<=?2 ORDER BY first DESC LIMIT 1) AND last>=?2 AND id>?3", params![kind,key,after], read).optional()?;
        let large = large::candidate(c, kind, key, after)?;
        if let Some(large) = large.filter(|id| normal.as_ref().is_none_or(|(n, _, _, _)| id < n)) {
            Some(c.query_row(
                "SELECT id,kind,count,payload FROM dictionary_blocks WHERE id=?1",
                [large],
                read,
            )?)
        } else {
            normal
        }
    } else if let Some(kind) = find(1) {
        c.query_row("SELECT id,kind,count,payload FROM dictionary_blocks WHERE kind=?1 AND id>?2 ORDER BY id LIMIT 1", params![kind,after], read).optional()?
    } else {
        c.query_row(
            "SELECT id,kind,count,payload FROM dictionary_blocks WHERE id>?1 ORDER BY id LIMIT 1",
            [after],
            read,
        )
        .optional()?
    };
    let Some((id, kind, count, bytes)) = block else {
        return Ok(None);
    };
    Ok(Some((
        id,
        codec::decode(&bytes, count)?
            .into_iter()
            .map(|(id, key)| {
                vec![
                    Value::Integer(id),
                    Value::Text(kind.clone()),
                    Value::Text(key),
                ]
            })
            .collect(),
    )))
}

fn read(r: &rusqlite::Row<'_>) -> Result<(i64, String, usize, Vec<u8>)> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
}

fn store(c: &Connection, id: i64, kind: &str, values: &[(i64, String)]) -> Result<i64> {
    let Some((_, first)) = values.first() else {
        return Ok(id);
    };
    let last = &values[values.len() - 1].1;
    let large = values.len() == 1 && first.len() > 65536;
    let digest = large.then(|| blake3::hash(first.as_bytes()));
    c.execute("INSERT INTO dictionary_blocks(id,kind,first,last,count,payload,digest) VALUES(nullif(?1,0),?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET first=excluded.first,last=excluded.last,count=excluded.count,payload=excluded.payload,digest=excluded.digest",params![id,kind,(!large).then_some(first),(!large).then_some(last),values.len(),codec::encode(values),digest.as_ref().map(|h|h.as_bytes().as_slice())])?;
    let id = if id == 0 { c.last_insert_rowid() } else { id };
    let ids = serde_json::to_string(&values.iter().map(|(id, _)| *id).collect::<Vec<_>>())
        .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    c.execute("INSERT INTO dictionary_ids SELECT value,?2 FROM json_each(?1) WHERE true ON CONFLICT(id) DO UPDATE SET block=excluded.block WHERE dictionary_ids.block<>excluded.block",params![ids,id])?;
    Ok(id)
}
