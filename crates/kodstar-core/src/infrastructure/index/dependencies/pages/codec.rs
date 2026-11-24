// Purpose: 안정된 slot과 여러 lookup 목록을 하나의 제한된 페이지로 인코딩.
// Pattern: Bounded Codec.
// Usage: page cursor·변경·일괄 공개가 같은 정규 표현을 사용.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/pages/mod.rs, crates/kodstar-core/src/infrastructure/index/dependencies/codec.rs.

use crate::infrastructure::index::packed::codec::{self, Input};
use rusqlite::Result;

pub(super) const SLOTS: usize = 64;
pub(super) const MAX_BYTES: usize = 64 * 1024;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(in crate::infrastructure::index) struct Row {
    pub slot: usize,
    pub key: i64,
    pub part: i64,
    pub count: usize,
    pub payload: Vec<u8>,
}

pub(super) fn encode(group: i64, rows: &[Row]) -> Result<Vec<u8>> {
    if group < 0 || rows.is_empty() || rows.len() > SLOTS {
        return Err(codec::invalid());
    }
    let mut bytes = Vec::new();
    let mut previous = None;
    let mut identities = Vec::with_capacity(rows.len());
    for row in rows {
        let offset = row
            .key
            .checked_sub(group.checked_mul(128).ok_or_else(codec::invalid)?)
            .filter(|v| (1..=128).contains(v))
            .ok_or_else(codec::invalid)?;
        if row.slot >= SLOTS || previous.is_some_and(|p| p >= row.slot) || row.part < 0 {
            return Err(codec::invalid());
        }
        super::super::codec::visit(&row.payload, row.count, |_, _| Ok(()))
            .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
        codec::integer(&mut bytes, row.slot as u64);
        codec::integer(&mut bytes, offset as u64);
        codec::integer(&mut bytes, row.part as u64);
        codec::integer(&mut bytes, row.count as u64);
        codec::integer(&mut bytes, row.payload.len() as u64);
        bytes.extend_from_slice(&row.payload);
        previous = Some(row.slot);
        identities.push((row.key, row.part));
    }
    if bytes.len() > MAX_BYTES {
        return Err(codec::invalid());
    }
    identities.sort_unstable();
    if identities.windows(2).any(|w| w[0] == w[1]) {
        return Err(codec::invalid());
    }
    Ok(bytes)
}

pub(super) fn decode(group: i64, count: usize, bytes: &[u8]) -> Result<Vec<Row>> {
    let mut rows = Vec::with_capacity(count.min(SLOTS));
    visit(group, count, bytes, |slot, key, part, count, payload| {
        rows.push(Row {
            slot,
            key,
            part,
            count,
            payload: payload.to_vec(),
        });
        Ok(())
    })?;
    Ok(rows)
}

pub(super) fn visit(
    group: i64,
    count: usize,
    bytes: &[u8],
    mut consume: impl FnMut(usize, i64, i64, usize, &[u8]) -> Result<()>,
) -> Result<()> {
    if group < 0 || count == 0 || count > SLOTS || bytes.len() > MAX_BYTES {
        return Err(codec::invalid());
    }
    let mut input = Input(bytes);
    let mut identities = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let slot = usize::try_from(input.integer()?).map_err(|_| codec::invalid())?;
        let offset = input.id()?;
        let part = input.id()?;
        let count = usize::try_from(input.integer()?).map_err(|_| codec::invalid())?;
        let size = usize::try_from(input.integer()?).map_err(|_| codec::invalid())?;
        if slot >= SLOTS
            || !(1..=128).contains(&offset)
            || !(1..=128).contains(&count)
            || size == 0
            || size > 1280
            || previous.is_some_and(|p| p >= slot)
        {
            return Err(codec::invalid());
        }
        let payload = input.0.get(..size).ok_or_else(codec::invalid)?;
        input.0 = &input.0[size..];
        super::super::codec::visit(payload, count, |_, _| Ok(()))
            .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
        let key = group
            .checked_mul(128)
            .and_then(|v| v.checked_add(offset))
            .ok_or_else(codec::invalid)?;
        identities.push((key, part));
        consume(slot, key, part, count, payload)?;
        previous = Some(slot);
    }
    input.finish()?;
    identities.sort_unstable();
    if identities.windows(2).any(|w| w[0] == w[1]) {
        return Err(codec::invalid());
    }
    Ok(())
}

pub(super) fn rowid(page: i64, slot: usize) -> Result<i64> {
    page.checked_mul(SLOTS as i64)
        .and_then(|n| n.checked_add(slot as i64))
        .filter(|n| page > 0 && slot < SLOTS && *n > 0)
        .ok_or_else(codec::invalid)
}
