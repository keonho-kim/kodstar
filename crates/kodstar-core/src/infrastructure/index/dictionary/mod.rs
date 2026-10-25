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

use super::packed::{self, Filter, Rows};
use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

mod publication;
pub(crate) mod routing;
pub(crate) use publication::publish;

pub(crate) const SCHEMA: &str = include_str!("schema.sql");

pub(super) fn scan(c: &Connection, after: i64, filters: &[Filter]) -> Result<Option<(i64, Rows)>> {
    let find = |column| filters.iter().find(|(k, _)| *k == column).map(|(_, v)| v);
    let block = if let Some(id) = find(0) {
        let id = match id {
            Value::Integer(id) => *id,
            _ => c.query_row("SELECT CAST(?1 AS INTEGER)", [id], |r| r.get(0))?,
        };
        if id > 0 {
            routing::find(c, id)?
                .filter(|id| *id > after)
                .map(|block| {
                    c.query_row(
                        "SELECT id,kind,count,payload FROM dictionary_blocks WHERE id=?1",
                        [block],
                        read,
                    )
                })
                .transpose()?
        } else {
            None
        }
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
    let keys = codec::select(&bytes, count, |id, key| {
        packed::key_matches(filters, id, &kind, key)
    })?;
    Ok(Some((id, Rows::Keys { kind, keys })))
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
    let mut old = Vec::new();
    if id != 0 {
        let previous: Option<(usize, Vec<u8>)> = c
            .query_row(
                "SELECT count,payload FROM dictionary_blocks WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((count, bytes)) = previous {
            codec::select(&bytes, count, |id, _| {
                old.push(id);
                false
            })?;
        }
    }
    c.execute("INSERT INTO dictionary_blocks(id,kind,first,last,count,payload,digest) VALUES(nullif(?1,0),?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET first=excluded.first,last=excluded.last,count=excluded.count,payload=excluded.payload,digest=excluded.digest",params![id,kind,(!large).then_some(first),(!large).then_some(last),values.len(),codec::encode(values),digest.as_ref().map(|h|h.as_bytes().as_slice())])?;
    let id = if id == 0 { c.last_insert_rowid() } else { id };
    routing::update(
        c,
        id,
        &old,
        &values.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
    )?;
    Ok(id)
}
