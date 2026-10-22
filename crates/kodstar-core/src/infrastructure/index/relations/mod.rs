// Purpose: 관계 압축 블록의 검색·원자적 변경과 행 ID 보존.
// Pattern: Storage Adapter.
// Usage: resolver publication과 SQLite virtual table의 쓰기 callback이 호출.
// Related: crates/kodstar-core/src/infrastructure/index/relations/schema.sql, crates/kodstar-core/src/infrastructure/index/relations/codec.rs.

mod codec;
mod publication;
mod removal;
mod verification;
pub(crate) use removal::partial;
pub(crate) use verification::verify;
#[cfg(test)]
mod tests;
pub(crate) use publication::publish;

use super::packed::{Filter, Row, codec::invalid};
use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

pub(crate) const SCHEMA: &str = include_str!("schema.sql");

pub(super) fn scan(
    c: &Connection,
    after: i64,
    filters: &[Filter],
) -> Result<Option<(i64, Vec<Row>)>> {
    let find = |col| filters.iter().find(|(k, _)| *k == col).map(|(_, v)| v);
    let block = if let Some(id) = find(0) {
        c.query_row(
            "SELECT block FROM relation_ids WHERE id=?1 AND block>?2",
            params![id, after],
            |r| r.get(0),
        )
        .optional()?
    } else if let Some((column, value)) = filters.iter().find(|(col, _)| matches!(*col, 1 | 3 | 6))
    {
        c.query_row("SELECT block FROM relation_routes WHERE kind=?1 AND entity=?2 AND block>?3 ORDER BY block LIMIT 1",params![column,value,after],|r|r.get(0)).optional()?
    } else if let Some(source) = find(2) {
        c.query_row(
            "SELECT id FROM relation_blocks WHERE source_id=?1 AND id>?2 ORDER BY id LIMIT 1",
            params![source, after],
            |r| r.get(0),
        )
        .optional()?
    } else {
        c.query_row(
            "SELECT id FROM relation_blocks WHERE id>?1 ORDER BY id LIMIT 1",
            [after],
            |r| r.get(0),
        )
        .optional()?
    };
    block.map(|id| Ok((id, load(c, id)?.2))).transpose()
}

fn load(c: &Connection, id: i64) -> Result<(i64, i64, Vec<Row>)> {
    let (file, source, count, bytes): (i64, i64, usize, Vec<u8>) = c.query_row(
        "SELECT file_id,source_id,count,payload FROM relation_blocks WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok((file, source, codec::decode(&bytes, count, source)?))
}

pub(super) fn store(
    c: &Connection,
    block: i64,
    file: i64,
    source: i64,
    rows: &[Row],
) -> Result<i64> {
    if block != 0 {
        c.execute("DELETE FROM relation_ids WHERE block=?1", [block])?;
        c.execute("DELETE FROM relation_routes WHERE block=?1", [block])?;
    }
    if rows.is_empty() {
        c.execute("DELETE FROM relation_blocks WHERE id=?1", [block])?;
        return Ok(block);
    }
    c.execute("INSERT INTO relation_blocks(id,file_id,source_id,count,payload) VALUES(nullif(?1,0),?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET count=excluded.count,payload=excluded.payload",params![block,file,source,rows.len(),codec::encode(rows)?])?;
    let block = if block == 0 {
        c.last_insert_rowid()
    } else {
        block
    };
    // Only numeric routing fields cross this bounded batch; external source text
    // stays in the binary payload and is not copied into a transport value.
    let routes = rows
        .iter()
        .map(|row| -> Result<[Option<i64>; 5]> {
            let nullable = |column| {
                if row[column] == Value::Null {
                    Ok(None)
                } else {
                    codec::id(row, column).map(Some)
                }
            };
            Ok([
                Some(codec::id(row, 0)?),
                Some(codec::id(row, 1)?),
                nullable(3)?,
                Some(codec::id(row, 6)?),
                nullable(7)?,
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let routes = serde_json::to_string(&routes)
        .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    c.execute(
        "INSERT INTO relation_ids SELECT json_extract(value,'$[0]'),?2 FROM json_each(?1)",
        params![routes, block],
    )?;
    c.execute("INSERT OR IGNORE INTO relation_routes
        SELECT 1,json_extract(value,'$[1]'),?2 FROM json_each(?1)
        UNION ALL SELECT 3,json_extract(value,'$[2]'),?2 FROM json_each(?1) WHERE json_extract(value,'$[2]') IS NOT NULL
        UNION ALL SELECT 6,json_extract(value,'$[3]'),?2 FROM json_each(?1)
        UNION ALL SELECT 6,json_extract(value,'$[4]'),?2 FROM json_each(?1) WHERE json_extract(value,'$[4]') IS NOT NULL",params![routes,block])?;
    Ok(block)
}

pub(super) fn remove(c: &Connection, id: i64) -> Result<()> {
    let block: Option<i64> = c
        .query_row("SELECT block FROM relation_ids WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(block) = block {
        let (file, source, mut rows) = load(c, block)?;
        rows.retain(|r| r[0] != Value::Integer(id));
        store(c, block, file, source, &rows)?;
    }
    Ok(())
}

pub(super) fn insert(c: &Connection, mut row: Row) -> Result<i64> {
    if row.len() != 8 {
        return Err(invalid());
    }
    if row[0] == Value::Null {
        let next: i64 = c.query_row("SELECT coalesce(max(id),0)+1 FROM relation_ids", [], |r| {
            r.get(0)
        })?;
        row[0] = Value::Integer(next);
    }
    let id = codec::id(&row, 0)?;
    let fact = codec::id(&row, 1)?;
    let source = codec::id(&row, 2)?;
    let file: i64 = c.query_row("SELECT file_id FROM facts WHERE id=?1", [fact], |r| {
        r.get(0)
    })?;
    for column in [2, 3] {
        if row[column] != Value::Null
            && !c.query_row(
                "SELECT EXISTS(SELECT 1 FROM symbols WHERE id=?1)",
                [codec::id(&row, column)?],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Err(invalid());
        }
    }
    for column in [6, 7] {
        if row[column] != Value::Null
            && !c.query_row(
                "SELECT EXISTS(SELECT 1 FROM relation_texts WHERE id=?1)",
                [codec::id(&row, column)?],
                |r| r.get::<_, bool>(0),
            )?
        {
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
    let size = |rows: &[Row]| {
        rows.iter()
            .flat_map(|r| r.iter())
            .map(|v| if let Value::Text(s) = v { s.len() } else { 8 })
            .sum::<usize>()
    };
    if !rows.is_empty() && size(&rows) + size(std::slice::from_ref(&row)) > 65536 {
        block = 0;
        rows.clear();
    }
    rows.push(row);
    rows.sort_by_key(|r| match r[0] {
        Value::Integer(id) => id,
        _ => 0,
    });
    store(c, block, file, source, &rows)?;
    Ok(id)
}
