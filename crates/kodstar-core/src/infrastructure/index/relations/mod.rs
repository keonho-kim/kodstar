// Purpose: 관계 압축 블록의 검색·원자적 변경과 행 ID 보존.
// Pattern: Storage Adapter.
// Usage: resolver publication과 SQLite virtual table의 쓰기 callback이 호출.
// Related: crates/kodstar-core/src/infrastructure/index/relations/schema.sql, crates/kodstar-core/src/infrastructure/index/relations/codec.rs.

mod codec;
mod decode;
mod merge;
mod prior;
pub(super) use prior::SCHEMA as PRIOR_SCHEMA;
mod row;
mod writer;
pub(crate) use row::Relation;
use writer::Writer;
mod mutation;
mod publication;
mod removal;
pub(crate) use mutation::{clear_bodies, remove_targets};
mod verification;
pub(crate) use removal::partial;
pub(crate) use verification::verify;
#[cfg(test)]
mod tests;
pub(crate) use publication::publish;

use super::packed::{Filter, Rows, codec::invalid};
use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

pub(crate) const SCHEMA: &str = include_str!("schema.sql");

pub(super) fn scan(c: &Connection, after: i64, filters: &[Filter]) -> Result<Option<(i64, Rows)>> {
    let find = |col| filters.iter().find(|(k, _)| *k == col).map(|(_, v)| v);
    let block = if let Some(id) = find(0) {
        c.query_row(
            "SELECT block FROM relation_ids WHERE id=?1 AND block>?2",
            params![id, after],
            |r| r.get(0),
        )
        .optional()?
    } else if let Some(value) = find(1) {
        route(c, 1, value, after)?
    } else if let Some(source) = find(2) {
        c.query_row(
            "SELECT id FROM relation_blocks WHERE source_id=?1 AND id>?2 ORDER BY id LIMIT 1",
            params![source, after],
            |r| r.get(0),
        )
        .optional()?
    } else if let Some(value) = find(3) {
        route(c, 3, value, after)?
    } else if let Some(value) = find(6) {
        route(c, 6, value, after)?
    } else {
        c.query_row(
            "SELECT id FROM relation_blocks WHERE id>?1 ORDER BY id LIMIT 1",
            [after],
            |r| r.get(0),
        )
        .optional()?
    };
    block
        .map(|id| {
            let (_, source, count, bytes) = block_data(c, id)?;
            Ok((
                id,
                Rows::Relations(codec::select(&bytes, count, source, filters)?),
            ))
        })
        .transpose()
}

fn block_data(c: &Connection, id: i64) -> Result<(i64, i64, usize, Vec<u8>)> {
    c.query_row(
        "SELECT file_id,source_id,count,payload FROM relation_blocks WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
}

fn load(c: &Connection, id: i64) -> Result<(i64, i64, Vec<Relation>)> {
    let (file, source, count, bytes) = block_data(c, id)?;
    Ok((file, source, codec::decode(&bytes, count, source)?))
}

fn store(c: &Connection, block: i64, file: i64, source: i64, rows: &[Relation]) -> Result<i64> {
    Writer::new(c)?.store(block, file, source, rows)
}

pub(super) fn remove(c: &Connection, id: i64) -> Result<()> {
    let block: Option<i64> = c
        .query_row("SELECT block FROM relation_ids WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(block) = block {
        let (file, source, mut rows) = load(c, block)?;
        rows.retain(|r| r.id != id);
        store(c, block, file, source, &rows)?;
    }
    Ok(())
}

pub(super) fn insert(c: &Connection, mut values: Vec<Value>) -> Result<i64> {
    if values.len() != 8 {
        return Err(invalid());
    }
    if values[0] == Value::Null {
        let next: i64 = c.query_row(prior::MAX_ID, [], |r| r.get(0))?;
        values[0] = Value::Integer(next.checked_add(1).ok_or_else(invalid)?);
    }
    let row = Relation::from_values(values)?;
    let (id, source) = (row.id, row.source);
    let file: i64 = c.query_row("SELECT file_id FROM facts WHERE id=?1", [row.fact], |r| {
        r.get(0)
    })?;
    for target in [Some(source), row.target].into_iter().flatten() {
        if !c.query_row(
            "SELECT EXISTS(SELECT 1 FROM symbols WHERE id=?1)",
            [target],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(invalid());
        }
    }
    for text in [Some(row.rule), row.reason].into_iter().flatten() {
        if !c.query_row(
            "SELECT EXISTS(SELECT 1 FROM relation_texts WHERE id=?1)",
            [text],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(invalid());
        }
    }
    let block:Option<i64>=c.query_row("SELECT id FROM relation_blocks WHERE file_id=?1 AND source_id=?2 AND count<128 ORDER BY id DESC LIMIT 1",params![file,source],|r|r.get(0)).optional()?;
    let mut block = block.unwrap_or(0);
    let mut rows = if block == 0 {
        Vec::new()
    } else {
        load(c, block)?.2
    };
    if !rows.is_empty()
        && rows.iter().map(Relation::owned_bytes).sum::<usize>() + row.owned_bytes() > 65536
    {
        block = 0;
        rows.clear();
    }
    rows.push(row);
    rows.sort_by_key(|r| r.id);
    store(c, block, file, source, &rows)?;
    Ok(id)
}

fn route(c: &Connection, kind: usize, value: &Value, after: i64) -> Result<Option<i64>> {
    c.query_row("SELECT block FROM relation_routes WHERE kind=?1 AND entity=?2 AND block>?3 ORDER BY block LIMIT 1",
        params![kind, value, after], |r| r.get(0)).optional()
}
