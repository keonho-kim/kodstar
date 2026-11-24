// Purpose: 관계 역조회 페이지·블록 key의 독립 복원과 테스트 변경 함수.
// Pattern: Reference Decoder.
// Usage: packed 지원이 일반 SQLite 연결에서 압축 schema를 읽고 표적 변경.
// Related: crates/kodstar-core/tests/support/packed.rs, crates/kodstar-core/tests/support/packed_mutations.rs.

use super::number;
use rusqlite::{Connection, Result, functions::FunctionFlags};

fn slots(mut bytes: &[u8]) -> Result<Vec<i64>> {
    let mut slots = Vec::new();
    let mut at = -1i64;
    while !bytes.is_empty() {
        let gap = i64::try_from(number(&mut bytes)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
        at = at
            .checked_add(gap)
            .filter(|n| gap > 0 && *n < 1024)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        slots.push(at);
    }
    Ok(slots)
}

fn integer(bytes: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        bytes.push(n as u8 | 128);
        n >>= 7;
    }
    bytes.push(n as u8);
}

fn pairs(mut bytes: &[u8]) -> Result<Vec<[u64; 2]>> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        let kind = number(&mut bytes)?;
        let count = number(&mut bytes)?;
        if count > 256 || result.len() as u64 + count > 512 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut entity = 0u64;
        for _ in 0..count {
            entity = entity
                .checked_add(number(&mut bytes)?)
                .ok_or(rusqlite::Error::InvalidQuery)?;
            result.push([kind, entity]);
        }
    }
    Ok(result)
}

pub(super) fn register(c: &Connection, flags: FunctionFlags) -> Result<()> {
    super::expansion::register(c, c"kodstar_relation_pair_rows", |bytes, _| {
        pairs(bytes)?
            .into_iter()
            .map(|[a, b]| {
                Ok([
                    i64::try_from(a).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    i64::try_from(b).map_err(|_| rusqlite::Error::InvalidQuery)?,
                ])
            })
            .collect()
    })?;
    super::expansion::register(c, c"kodstar_relation_posting_rows", |bytes, page| {
        slots(bytes)?
            .into_iter()
            .map(|slot| {
                Ok([
                    page.checked_mul(1024)
                        .and_then(|n| n.checked_add(slot + 1))
                        .ok_or(rusqlite::Error::InvalidQuery)?,
                    0,
                ])
            })
            .collect()
    })?;
    c.create_scalar_function("kodstar_relation_width", 0, flags, |_| Ok(1024i64))?;
    c.create_scalar_function("kodstar_relation_page", 3, flags, |ctx| {
        let mut values = slots(ctx.get_raw(0).as_blob()?)?;
        let slot: i64 = ctx.get(1)?;
        let add: bool = ctx.get(2)?;
        if !(0..1024).contains(&slot) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        values.retain(|n| *n != slot);
        if add {
            values.push(slot);
            values.sort_unstable();
        }
        let mut bytes = Vec::new();
        let mut previous = -1;
        for value in values {
            integer(&mut bytes, (value - previous) as u64);
            previous = value;
        }
        Ok(bytes)
    })
}
