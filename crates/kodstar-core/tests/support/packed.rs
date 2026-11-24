// Purpose: 제품 virtual table과 독립된 압축 인덱스 기준 복원.
// Pattern: Reference Decoder.
// Usage: 통합 테스트가 quiescent 인덱스를 일반 SQLite 연결로 비교.
// Related: crates/kodstar-core/tests/support/postings.rs, crates/kodstar-core/tests/support/packed_mutations.rs.

use rusqlite::{Connection, Result, functions::FunctionFlags};
#[path = "../../src/infrastructure/index/packed/expansion.rs"]
pub(super) mod expansion;
mod relations;
mod routing;

pub(super) fn number(bytes: &mut &[u8]) -> Result<u64> {
    let mut value = 0;
    for shift in (0..=63).step_by(7) {
        let (byte, rest) = bytes.split_first().ok_or(rusqlite::Error::InvalidQuery)?;
        if shift == 63 && *byte > 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        value |= u64::from(byte & 127) << shift;
        *bytes = rest;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    Err(rusqlite::Error::InvalidQuery)
}

fn text(bytes: &mut &[u8]) -> Result<String> {
    let size = number(bytes)? as usize;
    let raw = bytes.get(..size).ok_or(rusqlite::Error::InvalidQuery)?;
    let text = std::str::from_utf8(raw)
        .map_err(|_| rusqlite::Error::InvalidQuery)?
        .to_owned();
    *bytes = &bytes[size..];
    Ok(text)
}

pub(super) fn keys(mut bytes: &[u8], count: usize) -> Result<Vec<(u64, String)>> {
    let mut previous = String::new();
    let mut rows = Vec::new();
    for _ in 0..count {
        let id = number(&mut bytes)?;
        let prefix = number(&mut bytes)? as usize;
        let mut key = previous
            .get(..prefix)
            .ok_or(rusqlite::Error::InvalidQuery)?
            .to_owned();
        key.push_str(&text(&mut bytes)?);
        previous.clone_from(&key);
        rows.push((id, key));
    }
    if !bytes.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(rows)
}

pub fn register(c: &Connection) -> Result<()> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='dictionary_blocks')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let flags = FunctionFlags::SQLITE_UTF8
        | FunctionFlags::SQLITE_DETERMINISTIC
        | FunctionFlags::SQLITE_INNOCUOUS;
    relations::register(c, flags)?;
    routing::register(c, flags)?;
    expansion::register(c, c"kodstar_dictionary_route_rows", |bytes, _| {
        routes(bytes)
    })?;
    c.create_scalar_function("kodstar_route_at", 2, flags, |ctx| {
        let slot: usize = ctx.get(1)?;
        if slot >= 128 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(routes(ctx.get_raw(0).as_blob()?)?
            .iter()
            .find(|r| r[0] == slot as i64)
            .map_or(0, |r| r[1]))
    })?;
    c.create_scalar_function("reference_keys", 2, flags, |ctx| {
        serde_json::to_string(&keys(ctx.get_raw(0).as_blob()?, ctx.get(1)?)?)
            .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
    })?;
    c.execute_batch(&format!("CREATE TEMP VIEW IF NOT EXISTS lookup_keys AS SELECT json_extract(j.value,'$[0]') id,b.kind,json_extract(j.value,'$[1]') lookup_key FROM dictionary_blocks b,json_each(reference_keys(b.payload,b.count)) j;
        CREATE TEMP VIEW IF NOT EXISTS lookup_dependencies AS SELECT d.fact_id,k.kind,k.lookup_key,d.found FROM fact_lookups d JOIN lookup_keys k ON k.id=d.key_id;
        CREATE TEMP VIEW IF NOT EXISTS relationship_rows AS SELECT json_extract(j.value,'$[0]') id,json_extract(j.value,'$[1]') fact_id,json_extract(j.value,'$[2]') source_id,json_extract(j.value,'$[3]') target_id,json_extract(j.value,'$[4]') external_key,json_extract(j.value,'$[5]') resolution,json_extract(j.value,'$[6]') rule_id,json_extract(j.value,'$[7]') reason_id FROM relation_blocks b,json_each(reference_edges(b.payload,b.count,b.source_id,{})) j;
        CREATE TEMP VIEW IF NOT EXISTS relationships AS SELECT r.id,r.fact_id,r.source_id,r.target_id,r.external_key,r.resolution,(SELECT value FROM relation_texts WHERE id=r.rule_id) rule,(SELECT value FROM relation_texts WHERE id=r.reason_id) reason FROM relationship_rows r;",relations::POOL))
}

fn routes(mut bytes: &[u8]) -> Result<Vec<[i64; 2]>> {
    let mut rows = Vec::new();
    let mut at = 0usize;
    while at < 128 {
        let n = usize::try_from(number(&mut bytes)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let block =
            i64::try_from(number(&mut bytes)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let end = at
            .checked_add(n)
            .filter(|end| n > 0 && *end <= 128)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        if block > 0 {
            for slot in at..end {
                rows.push([slot as i64, block]);
            }
        }
        at = end;
    }
    if !bytes.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(rows)
}
