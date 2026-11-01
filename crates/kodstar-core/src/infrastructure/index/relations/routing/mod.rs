// Purpose: 관계 역조회의 압축 페이지와 블록 측 검색 key 관리.
// Pattern: Paged Routing.
// Usage: writer·SQLite trigger·packed cursor·doctor가 같은 transaction을 사용.
// Related: crates/kodstar-core/src/infrastructure/index/relations/routing/schema.sql, crates/kodstar-core/src/infrastructure/index/relations/writer.rs.

mod codec;
mod cursor;
pub(in crate::infrastructure::index) use cursor::Cursor;
#[cfg(test)]
mod tests;

use super::Relation;
use crate::infrastructure::index::packed::codec as packed;
use rusqlite::{Connection, Result, functions::FunctionFlags};
use std::collections::BTreeSet;

pub(super) fn encode(rows: &[Relation]) -> Result<Vec<u8>> {
    let mut pairs = BTreeSet::new();
    for row in rows {
        row.validate()?;
        for (kind, entity) in [
            (1, Some(row.fact)),
            (3, row.target),
            (6, Some(row.rule)),
            (6, row.reason),
        ] {
            if let Some(entity) = entity {
                pairs.insert((kind, entity));
            }
        }
    }
    let mut bytes = Vec::new();
    for kind in [1, 3, 6] {
        let values = pairs
            .range((kind, 0)..=(kind, i64::MAX))
            .map(|(_, entity)| *entity)
            .collect::<Vec<_>>();
        if values.is_empty() {
            continue;
        }
        packed::integer(&mut bytes, kind as u64);
        packed::integer(&mut bytes, values.len() as u64);
        let mut previous = 0;
        for entity in values {
            packed::integer(&mut bytes, (entity - previous) as u64);
            previous = entity;
        }
    }
    Ok(bytes)
}

pub(super) fn register(c: &Connection) -> Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8
        | FunctionFlags::SQLITE_DETERMINISTIC
        | FunctionFlags::SQLITE_INNOCUOUS;
    c.create_scalar_function("kodstar_relation_width", 0, flags, |_| Ok(codec::WIDTH))?;
    c.create_scalar_function("kodstar_relation_page", 3, flags, |ctx| {
        codec::change(ctx.get_raw(0).as_blob()?, ctx.get(1)?, ctx.get(2)?)
    })?;
    c.create_scalar_function("kodstar_relation_postings", 2, flags, |ctx| {
        let page = ctx.get(1)?;
        let ids = codec::decode(ctx.get_raw(0).as_blob()?)?
            .into_iter()
            .map(|slot| codec::absolute(page, slot))
            .collect::<Result<Vec<_>>>()?;
        serde_json::to_string(&ids).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
    })?;
    c.create_scalar_function("kodstar_relation_pairs", 1, flags, |ctx| {
        let pairs = codec::pairs(ctx.get_raw(0).as_blob()?)?;
        serde_json::to_string(&pairs).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
    })
}

pub(super) fn verify(c: &Connection) -> Result<()> {
    let mut query = c.prepare("SELECT page,payload FROM relation_route_pages")?;
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        let page = row.get(0)?;
        let values = codec::decode(row.get_ref(1)?.as_blob()?)?;
        if values.is_empty() {
            return Err(packed::invalid());
        }
        for slot in values {
            codec::absolute(page, slot)?;
        }
    }
    Ok(())
}
