// Purpose: 제한된 역조회 페이지의 차이값과 블록의 정렬된 조회 key 인코딩.
// Pattern: Bounded Encoding.
// Usage: routing SQL 함수와 writer·doctor가 동일한 바이트 경계를 사용.
// Related: crates/kodstar-core/src/infrastructure/index/relations/routing/mod.rs.

use crate::infrastructure::index::packed::codec::{self, Input};
use rusqlite::Result;

pub(super) const WIDTH: i64 = 1024;

pub(super) fn decode(bytes: &[u8]) -> Result<Vec<i64>> {
    let mut input = Input(bytes);
    let mut values = Vec::new();
    let mut previous = -1i64;
    while !input.0.is_empty() {
        let gap = input.id()?;
        let next = previous.checked_add(gap).ok_or_else(codec::invalid)?;
        if gap == 0 || next >= WIDTH || values.len() >= WIDTH as usize {
            return Err(codec::invalid());
        }
        values.push(next);
        previous = next;
    }
    Ok(values)
}

pub(super) fn change(bytes: &[u8], slot: i64, add: bool) -> Result<Vec<u8>> {
    if !(0..WIDTH).contains(&slot) {
        return Err(codec::invalid());
    }
    let mut values = decode(bytes)?;
    match (values.binary_search(&slot), add) {
        (Err(at), true) => values.insert(at, slot),
        (Ok(at), false) => {
            values.remove(at);
        }
        _ => {}
    }
    let mut bytes = Vec::new();
    let mut previous = -1;
    for value in values {
        codec::integer(&mut bytes, (value - previous) as u64);
        previous = value;
    }
    Ok(bytes)
}

pub(super) fn absolute(page: i64, slot: i64) -> Result<i64> {
    page.checked_mul(WIDTH)
        .and_then(|n| n.checked_add(slot))
        .and_then(|n| n.checked_add(1))
        .filter(|n| *n > 0)
        .ok_or_else(codec::invalid)
}

pub(super) fn pairs(bytes: &[u8]) -> Result<Vec<(i64, i64)>> {
    let mut input = Input(bytes);
    let mut pairs = Vec::new();
    let mut previous_kind = 0;
    while !input.0.is_empty() {
        let kind = input.id()?;
        let count = input.integer()?;
        if !matches!(kind, 1 | 3 | 6)
            || kind <= previous_kind
            || count == 0
            || count > 256
            || pairs.len() as u64 + count > 512
        {
            return Err(codec::invalid());
        }
        let mut entity = 0i64;
        for _ in 0..count {
            let gap = input.id()?;
            entity = entity.checked_add(gap).ok_or_else(codec::invalid)?;
            if gap == 0 {
                return Err(codec::invalid());
            }
            pairs.push((kind, entity));
        }
        previous_kind = kind;
    }
    Ok(pairs)
}
