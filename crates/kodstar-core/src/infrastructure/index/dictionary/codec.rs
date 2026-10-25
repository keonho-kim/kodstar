// Purpose: 32개 조회 key의 원문·ID를 접두어 차이로 손실 없이 표현.
// Pattern: Front Coding.
// Usage: dictionary가 정렬된 UTF-8 key 블록을 저장·복원.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs, crates/kodstar-core/src/infrastructure/index/packed/codec.rs.

use super::super::packed::codec::{self, Input};
use rusqlite::Result;

pub(super) fn encode(rows: &[(i64, String)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut previous = "";
    for (id, key) in rows {
        let mut shared = previous
            .bytes()
            .zip(key.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !key.is_char_boundary(shared) {
            shared -= 1;
        }
        codec::integer(&mut bytes, *id as u64);
        codec::integer(&mut bytes, shared as u64);
        codec::text(&mut bytes, &key[shared..]);
        previous = key;
    }
    bytes
}

pub(super) fn decode(bytes: &[u8], count: usize) -> Result<Vec<(i64, String)>> {
    select(bytes, count, |_, _| true)
}

pub(super) fn select(
    bytes: &[u8],
    count: usize,
    mut keep: impl FnMut(i64, &str) -> bool,
) -> Result<Vec<(i64, String)>> {
    if !(1..=32).contains(&count) {
        return Err(codec::invalid());
    }
    let mut input = Input(bytes);
    let mut rows = Vec::with_capacity(count);
    let mut previous = String::new();
    let mut decoded = 0usize;
    for at in 0..count {
        let id = input.id()?;
        let prefix = usize::try_from(input.integer()?).map_err(|_| codec::invalid())?;
        if id <= 0 || !previous.is_char_boundary(prefix) {
            return Err(codec::invalid());
        }
        let suffix = input.text_ref()?;
        decoded = decoded
            .checked_add(prefix)
            .and_then(|n| n.checked_add(suffix.len()))
            .ok_or_else(codec::invalid)?;
        if count > 1 && decoded > 65536 {
            return Err(codec::invalid());
        }
        // Compare before mutating the reused prefix buffer; retain only selected keys.
        if at > 0 && suffix.as_bytes() <= &previous.as_bytes()[prefix..] {
            return Err(codec::invalid());
        }
        previous.truncate(prefix);
        previous.push_str(suffix);
        if keep(id, &previous) {
            rows.push((id, previous.clone()));
        }
    }
    input.finish()?;
    Ok(rows)
}
