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
    if count == 0 || count > MAX_COUNT || bytes.len() > MAX_COUNT * 10 {
        return Err(invalid());
    }
    let mut output = String::with_capacity(count * 25 + 2);
    output.push('[');
    let mut offset = 0;
    let mut previous = 0u64;
    for index in 0..count {
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
        if index != 0 {
            output.push(',');
        }
        write!(&mut output, "[{fact},{}]", value & 1).map_err(|_| invalid())?;
        previous = fact;
    }
    if offset != bytes.len() {
        return Err(invalid());
    }
    output.push(']');
    Ok(output)
}

fn invalid() -> Error {
    Error::operation("invalid lookup dependency block")
}
