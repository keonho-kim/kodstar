// Purpose: lookup 논리 행의 안정 slot 변경과 페이지별 원자적 쓰기.
// Pattern: Single Writer.
// Usage: packed xUpdate와 전체 공개가 소유 transaction에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/pages/codec.rs, crates/kodstar-core/src/infrastructure/index/dependencies/pages/publication.rs.

use super::{Row, codec};
use crate::infrastructure::index::packed::{codec::invalid, statements};
use rusqlite::{Connection, OptionalExtension, Result, params, types::Value};

pub(super) fn load(c: &Connection, page: i64) -> Result<(i64, i64, Vec<Row>)> {
    statements::row(
        c,
        "SELECT group_id,file_id,count,payload FROM lookup_pages WHERE id=?1",
        [page],
        |r| {
            let group = r.get(0)?;
            Ok((
                group,
                r.get(1)?,
                codec::decode(group, r.get(2)?, r.get_ref(3)?.as_blob()?)?,
            ))
        },
    )
}

pub(super) fn store(c: &Connection, page: i64, group: i64, file: i64, rows: &[Row]) -> Result<i64> {
    if rows.is_empty() {
        statements::execute(c, "DELETE FROM lookup_pages WHERE id=?1", [page])?;
        return Ok(page);
    }
    let bytes = codec::encode(group, rows)?;
    if page == 0 {
        let page: i64 = statements::row(
            c,
            "INSERT INTO lookup_pages(group_id,file_id,count,payload) VALUES(?1,?2,?3,?4) RETURNING id",
            params![group, file, rows.len(), bytes],
            |r| r.get(0),
        )?;
        codec::rowid(page, codec::SLOTS - 1)?;
        Ok(page)
    } else {
        statements::execute(
            c,
            "UPDATE lookup_pages SET count=?2,payload=?3 WHERE id=?1 AND (count<>?2 OR payload<>?3)",
            params![page, rows.len(), bytes],
        )?;
        Ok(page)
    }
}

fn input(values: Vec<Value>) -> Result<(i64, i64, Row)> {
    if values.len() != 6 {
        return Err(invalid());
    }
    let integer = |i: usize| {
        if let Value::Integer(n) = values[i] {
            Ok(n)
        } else {
            Err(invalid())
        }
    };
    let key = integer(1)?;
    let file = integer(2)?;
    let part = integer(3)?;
    let count = usize::try_from(integer(4)?).map_err(|_| invalid())?;
    if key <= 0 || file <= 0 || part < 0 {
        return Err(invalid());
    }
    let group = (key - 1) / 128;
    if values[0] != Value::Null && values[0] != Value::Integer(group) {
        return Err(invalid());
    }
    let Value::Blob(payload) = values.into_iter().nth(5).ok_or_else(invalid)? else {
        return Err(invalid());
    };
    super::super::codec::visit(&payload, count, |_, _| Ok(()))
        .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    Ok((
        group,
        file,
        Row {
            slot: 0,
            key,
            part,
            count,
            payload,
        },
    ))
}

fn guard(c: &Connection, key: i64) -> Result<()> {
    let valid: bool = statements::row(
        c,
        "SELECT EXISTS(SELECT 1 FROM dictionary_routes p JOIN dictionary_blocks b ON b.id=kodstar_route_at(p.payload,(?1-1)%128) WHERE p.id=(?1-1)/128)",
        [key],
        |r| r.get(0),
    )?;
    if !valid {
        return Err(invalid());
    }
    Ok(())
}

fn duplicate(c: &Connection, key: i64, file: i64, part: i64) -> Result<Option<i64>> {
    let group = (key - 1) / 128;
    let mut after = 0;
    loop {
        // Never prepare a query on our own virtual table during xUpdate. Besides
        // recursive mutable callbacks, caching it would retain a self-reference.
        let next = statements::row(c,
            "SELECT id,count,payload FROM lookup_pages WHERE group_id=?1 AND file_id=?2 AND id>?3 ORDER BY id LIMIT 1",
            params![group,file,after], |r| {
                let page = r.get(0)?;
                let mut found = None;
                codec::visit(group,r.get(1)?,r.get_ref(2)?.as_blob()?, |slot,candidate,segment,_,_| {
                    if key == candidate && part == segment { found = Some(codec::rowid(page,slot)?); }
                    Ok(())
                })?;
                Ok((page,found))
            }).optional()?;
        let Some((page, found)) = next else {
            return Ok(None);
        };
        if found.is_some() {
            return Ok(found);
        }
        after = page;
    }
}

pub(in crate::infrastructure::index) fn insert(c: &Connection, values: Vec<Value>) -> Result<i64> {
    let (group, file, row) = input(values)?;
    guard(c, row.key)?;
    if duplicate(c, row.key, file, row.part)?.is_some() {
        return Err(invalid());
    }
    append(c, group, file, row)
}

fn append(c: &Connection, group: i64, file: i64, mut row: Row) -> Result<i64> {
    let previous: Option<i64>=statements::row(c,"SELECT id FROM lookup_pages WHERE group_id=?1 AND file_id=?2 AND count<64 AND length(payload)+?3<=65536 ORDER BY id DESC LIMIT 1",params![group,file,row.payload.len()+40],|r|r.get(0)).optional()?;
    let (page, mut rows) = if let Some(page) = previous {
        (page, load(c, page)?.2)
    } else {
        (0, Vec::new())
    };
    row.slot = (0..codec::SLOTS)
        .find(|slot| rows.iter().all(|r| r.slot != *slot))
        .ok_or_else(invalid)?;
    let slot = row.slot;
    rows.push(row);
    rows.sort_by_key(|r| r.slot);
    let page = store(c, page, group, file, &rows)?;
    codec::rowid(page, slot)
}

pub(in crate::infrastructure::index) fn remove(c: &Connection, id: i64) -> Result<()> {
    if id < codec::SLOTS as i64 {
        return Err(invalid());
    }
    let page = id / codec::SLOTS as i64;
    let slot = id as usize % codec::SLOTS;
    let (group, file, mut rows) = load(c, page)?;
    let before = rows.len();
    rows.retain(|r| r.slot != slot);
    if rows.len() == before {
        return Err(invalid());
    }
    store(c, page, group, file, &rows)?;
    Ok(())
}

pub(in crate::infrastructure::index) fn update(
    c: &Connection,
    id: i64,
    values: Vec<Value>,
) -> Result<()> {
    let (group, file, mut row) = input(values)?;
    guard(c, row.key)?;
    if duplicate(c, row.key, file, row.part)?.is_some_and(|other| other != id) {
        return Err(invalid());
    }
    let page = id / codec::SLOTS as i64;
    let (old_group, old_file, mut rows) = load(c, page)?;
    let at = rows
        .iter()
        .position(|r| codec::rowid(page, r.slot).ok() == Some(id))
        .ok_or_else(invalid)?;
    row.slot = rows[at].slot;
    if group == old_group && file == old_file {
        rows[at] = row.clone();
        if codec::encode(group, &rows).is_ok() {
            store(c, page, group, file, &rows)?;
            return Ok(());
        }
    }
    remove(c, id)?;
    append(c, group, file, row)?;
    Ok(())
}
