// Purpose: 성공 bit와 정렬된 fact ID 차이의 정규 가변 정수 표현.
// Pattern: Bounded Encoding.
// Usage: dependency 블록 작성과 논리 view 함수가 최대 128행을 처리.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/mod.rs.

use crate::domain::diagnostics::{Error, Result};
use std::fmt::Write;

pub(super) const MAX_COUNT: usize = 128;

pub(super) fn push(output: &mut Vec<u8>, fact: i64, previous: i64, found: bool) -> Result<()> {
    if fact <= previous || previous < 0 {
        return Err(invalid());
    }
    let mut value = ((fact - previous) as u64) * 2 + u64::from(found);
    while value >= 128 {
        output.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    output.push(value as u8);
    Ok(())
}

pub(super) fn json(bytes: &[u8], count: usize) -> Result<String> {
    let mut output = String::with_capacity(count.min(MAX_COUNT) * 25 + 2);
    output.push('[');
    let mut first = true;
    visit(bytes, count, |fact, found| {
        if !first {
            output.push(',');
        }
        first = false;
        write!(&mut output, "[{fact},{}]", u8::from(found)).map_err(|_| invalid())
    })?;
    output.push(']');
    Ok(output)
}

pub(super) fn visit(
    bytes: &[u8],
    count: usize,
    mut consume: impl FnMut(i64, bool) -> Result<()>,
) -> Result<()> {
    if count == 0 || count > MAX_COUNT || bytes.len() > MAX_COUNT * 10 {
        return Err(invalid());
    }
    let mut offset = 0;
    let mut previous = 0u64;
    for _ in 0..count {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = *bytes.get(offset).ok_or_else(invalid)?;
            offset += 1;
            if shift == 63 && byte > 1 || shift > 63 || shift > 0 && byte == 0 {
                return Err(invalid());
            }
            value |= u64::from(byte & 127) << shift;
            if byte < 128 {
                break;
            }
            shift += 7;
        }
        let fact = previous.checked_add(value >> 1).ok_or_else(invalid)?;
        if fact <= previous || fact > i64::MAX as u64 {
            return Err(invalid());
        }
        consume(fact as i64, value & 1 == 1)?;
        previous = fact;
    }
    if offset != bytes.len() {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> Error {
    Error::operation("invalid lookup dependency block")
}
