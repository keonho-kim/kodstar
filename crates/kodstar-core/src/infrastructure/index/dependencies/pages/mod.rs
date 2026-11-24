// Purpose: 희소 lookup 페이지의 물리 저장과 기존 논리 행의 경계.
// Pattern: Paged Storage Adapter.
// Usage: packed virtual table과 dependency publication이 같은 transaction에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/pages/codec.rs, crates/kodstar-core/src/infrastructure/index/dependencies/schema.sql.

mod codec;
mod mutation;
pub(super) mod publication;
use crate::infrastructure::index::packed::{Filter, Rows, codec::invalid, statements};
pub(in crate::infrastructure::index) use codec::Row;
pub(in crate::infrastructure::index) use mutation::{insert, remove, update};
use rusqlite::{
    Connection, OptionalExtension, Result, params_from_iter,
    types::{Value, ValueRef},
};

pub(in crate::infrastructure::index) fn register(c: &Connection) -> Result<()> {
    super::super::packed::expansion::register(c, c"kodstar_lookup_page_keys", |bytes, parameter| {
        if parameter < 0 {
            return Err(invalid());
        }
        // Seven low bits hold the count; a group owns exactly 128 key IDs.
        let mut rows = Vec::new();
        codec::visit(
            parameter / 128,
            (parameter % 128 + 1) as usize,
            bytes,
            |_, key, _, _, _| {
                rows.push([key, 0]);
                Ok(())
            },
        )?;
        Ok(rows)
    })
}

pub(in crate::infrastructure::index) fn scan(
    c: &Connection,
    after: i64,
    filters: &[Filter],
) -> Result<Option<(i64, Rows)>> {
    let exact = |col| {
        filters.iter().find_map(|(k, v)| {
            if *k == col {
                if let Value::Integer(n) = v {
                    Some(*n)
                } else {
                    None
                }
            } else {
                None
            }
        })
    };
    let group = exact(1)
        .and_then(|key| (key > 0).then_some((key - 1) / 128))
        .or_else(|| exact(0));
    let file = exact(2);
    let mut sql =
        String::from("SELECT id,group_id,file_id,count,payload FROM lookup_pages WHERE id>?");
    let mut args = vec![after];
    if let Some(group) = group {
        sql.push_str(" AND group_id=?");
        args.push(group);
    }
    if let Some(file) = file {
        sql.push_str(" AND file_id=?");
        args.push(file);
    }
    sql.push_str(" ORDER BY id LIMIT 1");
    statements::row(c, &sql, params_from_iter(args), |r| {
        let page: i64 = r.get(0)?;
        let group: i64 = r.get(1)?;
        let file: i64 = r.get(2)?;
        let mut rows = Vec::new();
        codec::visit(
            group,
            r.get(3)?,
            r.get_ref(4)?.as_blob()?,
            |slot, key, part, count, payload| {
                if filters.iter().all(|(column, value)| {
                    let stored = match column {
                        0 => group,
                        1 => key,
                        2 => file,
                        3 => part,
                        _ => return true,
                    };
                    super::super::packed::matches(ValueRef::Integer(stored), value)
                }) {
                    rows.push(Row {
                        slot,
                        key,
                        part,
                        count,
                        payload: payload.to_vec(),
                    });
                }
                Ok(())
            },
        )?;
        Ok((
            page,
            Rows::Lookups {
                group,
                file,
                page,
                rows,
            },
        ))
    })
    .optional()
}

pub(in crate::infrastructure::index) fn value_at<'a>(
    group: i64,
    file: i64,
    row: &'a Row,
    column: usize,
) -> ValueRef<'a> {
    match column {
        0 => ValueRef::Integer(group),
        1 => ValueRef::Integer(row.key),
        2 => ValueRef::Integer(file),
        3 => ValueRef::Integer(row.part),
        4 => ValueRef::Integer(row.count as i64),
        5 => ValueRef::Blob(&row.payload),
        _ => ValueRef::Null,
    }
}

pub(in crate::infrastructure::index) fn identity(page: i64, row: &Row) -> Result<i64> {
    codec::rowid(page, row.slot)
}

pub(crate) fn clear_all(c: &Connection) -> Result<()> {
    if c.is_autocommit() {
        return Err(invalid());
    }
    c.execute("DELETE FROM lookup_pages", [])?;
    Ok(())
}

pub(crate) fn clear_file(c: &Connection, file: i64) -> Result<()> {
    c.execute("DELETE FROM lookup_pages WHERE file_id=?1", [file])?;
    Ok(())
}
