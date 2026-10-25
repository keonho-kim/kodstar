// Purpose: 고정 ID 그룹별 압축 블록 대응의 정확 조회·차이 갱신.
// Pattern: Paged Routing.
// Usage: dictionary 공개·회수·virtual table이 같은 transaction에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/routing/codec.rs, crates/kodstar-core/src/infrastructure/index/dictionary/schema.sql.

mod codec;
#[cfg(test)]
mod tests;
use rusqlite::{Connection, OptionalExtension, Result, functions::FunctionFlags, params};
use std::collections::BTreeSet;
const WIDTH: i64 = codec::WIDTH as i64;

fn absolute(group: i64, slot: usize) -> Result<i64> {
    group
        .checked_mul(WIDTH)
        .and_then(|n| n.checked_add(slot as i64 + 1))
        .filter(|n| *n > 0)
        .ok_or_else(|| rusqlite::Error::ModuleError("dictionary route ID overflow".into()))
}

pub(crate) fn register(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS dictionary_dirty_routes(id INTEGER PRIMARY KEY);",
    )?;
    let flags = FunctionFlags::SQLITE_UTF8
        | FunctionFlags::SQLITE_DETERMINISTIC
        | FunctionFlags::SQLITE_INNOCUOUS;
    c.create_scalar_function("kodstar_route_at", 2, flags, |ctx| {
        codec::at(ctx.get_raw(0).as_blob()?, ctx.get(1)?)
    })?;
    c.create_scalar_function("kodstar_routes", 1, flags, |ctx| {
        let page = codec::decode(ctx.get_raw(0).as_blob()?)?;
        let rows = page
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(i, n)| (i, *n))
            .collect::<Vec<_>>();
        serde_json::to_string(&rows).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
    })?;
    Ok(())
}

pub(super) fn find(c: &Connection, id: i64) -> Result<Option<i64>> {
    if id <= 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    c.query_row(
        "SELECT payload FROM dictionary_routes WHERE id=?1",
        [(id - 1) / WIDTH],
        |r| {
            let n = codec::at(r.get_ref(0)?.as_blob()?, ((id - 1) % WIDTH) as usize)?;
            Ok((n > 0).then_some(n))
        },
    )
    .optional()
    .map(Option::flatten)
}

pub(super) fn update(c: &Connection, block: i64, old: &[i64], new: &[i64]) -> Result<()> {
    if block <= 0 || old.iter().chain(new).any(|id| *id <= 0) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let groups = old
        .iter()
        .chain(new)
        .map(|id| (id - 1) / WIDTH)
        .collect::<BTreeSet<_>>();
    let mut read = c.prepare("SELECT payload FROM dictionary_routes WHERE id=?1")?;
    let mut write=c.prepare("INSERT INTO dictionary_routes(id,payload,last_id) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload,last_id=excluded.last_id WHERE dictionary_routes.payload<>excluded.payload")?;
    for group in groups {
        let mut page = read
            .query_row([group], |r| codec::decode(r.get_ref(0)?.as_blob()?))
            .optional()?
            .unwrap_or([0; codec::WIDTH]);
        for id in old.iter().filter(|id| (**id - 1) / WIDTH == group) {
            let slot = ((id - 1) % WIDTH) as usize;
            if page[slot] == block {
                page[slot] = 0;
            }
        }
        for id in new.iter().filter(|id| (**id - 1) / WIDTH == group) {
            page[((id - 1) % WIDTH) as usize] = block;
        }
        let last = page
            .iter()
            .rposition(|id| *id > 0)
            .map(|slot| absolute(group, slot))
            .transpose()?
            .unwrap_or(0);
        if write.execute(params![group, codec::encode(&page), last])? > 0 {
            c.execute(
                "INSERT OR IGNORE INTO dictionary_dirty_routes VALUES(?1)",
                [group],
            )?;
        }
    }
    Ok(())
}

pub(crate) fn collect(c: &Connection) -> Result<()> {
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM dictionary_dirty_routes d JOIN dictionary_routes p ON p.id=d.id CROSS JOIN lookup_rows l ON l.key_id BETWEEN p.id*128+1 AND min(p.id*128+128,9223372036854775807) LEFT JOIN dictionary_blocks b ON b.id=kodstar_route_at(p.payload,(l.key_id-1)%128) WHERE b.id IS NULL)",[],|r|r.get(0))?;
    if invalid {
        return Err(rusqlite::Error::ModuleError(
            "live lookup lost its dictionary route".into(),
        ));
    }
    c.execute("DELETE FROM dictionary_routes WHERE last_id=0", [])?;
    c.execute("DELETE FROM dictionary_dirty_routes", [])?;
    Ok(())
}

pub(super) fn verify(c: &Connection) -> Result<()> {
    let mut q = c.prepare("SELECT id,payload,last_id FROM dictionary_routes")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        let group: i64 = row.get(0)?;
        let expected: i64 = row.get(2)?;
        let page = codec::decode(row.get_ref(1)?.as_blob()?)?;
        let last = page
            .iter()
            .rposition(|id| *id > 0)
            .map(|slot| absolute(group, slot))
            .transpose()?
            .unwrap_or(0);
        if last != expected || last == 0 {
            return Err(rusqlite::Error::ModuleError(
                "dictionary route high-water mark mismatch".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn next(c: &Connection) -> Result<i64> {
    c.query_row(
        "SELECT COALESCE(MAX(last_id),0) FROM dictionary_routes",
        [],
        |r| r.get(0),
    )
}
