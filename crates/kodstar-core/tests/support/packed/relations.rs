// Purpose: 공유 후보 그룹과 개별 관계 연결의 독립 기준 복원.
// Pattern: Reference Decoder.
// Usage: packed 테스트 지원이 한 블록에 필요한 공유 payload만 SQL로 전달.
// Related: crates/kodstar-core/tests/support/packed.rs, crates/kodstar-core/tests/support/packed/routing.rs.

use super::{number, text};
use rusqlite::{Connection, Result, functions::FunctionFlags};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const POOL: &str =
    "(SELECT json_group_object(s.id,json_array(s.count,hex(s.payload))) FROM relation_sets s
    WHERE s.id IN (SELECT value FROM json_each(kodstar_relation_sets(b.payload,b.count))))";

fn associations(bytes: &mut &[u8], count: usize) -> Result<Vec<(u64, i64)>> {
    if !(1..=128).contains(&count) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut result = Vec::with_capacity(count);
    let (mut id, mut fact) = (0u64, 0i64);
    for _ in 0..count {
        let gap = number(bytes)?;
        id = id
            .checked_add(gap)
            .filter(|id| gap > 0 && *id <= i64::MAX as u64)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        let delta = number(bytes)?;
        fact = fact
            .checked_add(((delta >> 1) as i64) ^ -((delta & 1) as i64))
            .filter(|n| *n > 0)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        result.push((id, fact));
    }
    Ok(result)
}

fn target(bytes: &mut &[u8]) -> Result<Vec<Value>> {
    let id = number(bytes)?;
    let rule = number(bytes)?;
    let reason = number(bytes)?;
    let flags = number(bytes)?;
    let state = ["resolved", "candidate", "ambiguous", "unresolved"]
        .get((flags >> 1) as usize)
        .ok_or(rusqlite::Error::InvalidQuery)?;
    let external = if flags & 1 == 1 {
        Value::String(text(bytes)?)
    } else {
        Value::Null
    };
    Ok(vec![
        if id == 0 { Value::Null } else { json!(id) },
        external,
        json!(state),
        json!(rule),
        if reason == 0 {
            Value::Null
        } else {
            json!(reason)
        },
    ])
}

fn group(bytes: &mut &[u8], remaining: usize) -> Result<(usize, u64)> {
    let count = usize::try_from(number(bytes)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if count == 0 || count > remaining {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok((count, number(bytes)?))
}

fn references(mut bytes: &[u8], count: usize) -> Result<Vec<u64>> {
    associations(&mut bytes, count)?;
    let mut left = count;
    let mut refs = BTreeSet::new();
    while left > 0 {
        let (n, id) = group(&mut bytes, left)?;
        if id > 0 {
            refs.insert(id);
        } else {
            for _ in 0..n {
                target(&mut bytes)?;
            }
        }
        left -= n;
    }
    if !bytes.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(refs.into_iter().collect())
}

fn hex(text: &str) -> Result<Vec<u8>> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    pairs
        .iter()
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| rusqlite::Error::InvalidQuery)?;
            u8::from_str_radix(pair, 16).map_err(|_| rusqlite::Error::InvalidQuery)
        })
        .collect()
}

fn edges(mut bytes: &[u8], count: usize, source: i64, pool: &str) -> Result<Vec<Value>> {
    let pool: BTreeMap<String, (usize, String)> =
        serde_json::from_str(pool).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let ids = associations(&mut bytes, count)?;
    let mut rows = Vec::with_capacity(count);
    while rows.len() < count {
        let (n, id) = group(&mut bytes, count - rows.len())?;
        let targets = if id == 0 {
            (0..n)
                .map(|_| target(&mut bytes))
                .collect::<Result<Vec<_>>>()?
        } else {
            let (stored, payload) = pool
                .get(&id.to_string())
                .ok_or(rusqlite::Error::InvalidQuery)?;
            if *stored != n {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let payload = hex(payload)?;
            let mut input = payload.as_slice();
            let targets = (0..n)
                .map(|_| target(&mut input))
                .collect::<Result<Vec<_>>>()?;
            if !input.is_empty() {
                return Err(rusqlite::Error::InvalidQuery);
            }
            targets
        };
        for target in targets {
            let (identity, fact) = ids[rows.len()];
            let mut row = vec![json!(identity), json!(fact), json!(source)];
            row.extend(target);
            rows.push(Value::Array(row));
        }
    }
    if !bytes.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(rows)
}

pub(super) fn register(c: &Connection, flags: FunctionFlags) -> Result<()> {
    c.create_scalar_function("kodstar_relation_sets", 2, flags, |ctx| {
        serde_json::to_string(&references(ctx.get_raw(0).as_blob()?, ctx.get(1)?)?)
            .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
    })?;
    c.create_scalar_function("reference_edges", 4, flags, |ctx| {
        serde_json::to_string(&edges(
            ctx.get_raw(0).as_blob()?,
            ctx.get(1)?,
            ctx.get(2)?,
            &ctx.get::<String>(3)?,
        )?)
        .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
    })
}
