// Purpose: 제한된 원문 블록의 literal·copy 표현과 단일 기준 델타.
// Pattern: Bounded Encoding.
// Usage: 청크 adapter가 원문 주소와 별개인 물리 payload를 선택·복원.
// Related: crates/kodstar-core/src/infrastructure/history/content/chunks.rs, crates/kodstar-core/src/infrastructure/history/content/encoding/tests.rs.

use crate::domain::diagnostics::{Error, Result};

#[cfg(test)]
mod tests;

pub(in crate::infrastructure::history) const MAX: usize = 65536;
const SLOTS: usize = 8192;

pub(in crate::infrastructure::history) fn encode(input: &[u8], base: &[u8]) -> Vec<u8> {
    let mut table = [u32::MAX; SLOTS];
    for at in (0..base.len().saturating_sub(3)).step_by(4) {
        table[slot(&base[at..])] = at as u32;
    }
    let mut output = Vec::with_capacity(input.len());
    let (mut cursor, mut literal) = (0, 0);
    while cursor + 4 <= input.len() {
        let key = slot(&input[cursor..]);
        let candidate = table[key] as usize;
        table[key] = (base.len() + cursor) as u32;
        let mut matched = 0;
        if candidate < base.len() + cursor {
            while cursor + matched < input.len() {
                let source = candidate + matched;
                let byte = if source < base.len() {
                    base[source]
                } else {
                    input[source - base.len()]
                };
                if byte != input[cursor + matched] {
                    break;
                }
                matched += 1;
            }
        }
        if matched < 4 {
            cursor += 1;
            continue;
        }
        literals(&mut output, &input[literal..cursor]);
        integer(&mut output, ((matched - 4) as u64) * 2 + 1);
        integer(&mut output, candidate as u64);
        // One candidate per slot bounds collision work; every accepted match consumes input.
        cursor += matched;
        literal = cursor;
    }
    literals(&mut output, &input[literal..]);
    output
}

pub(in crate::infrastructure::history) fn decode(
    payload: &[u8],
    codec: u8,
    size: usize,
    base: &[u8],
) -> Result<Vec<u8>> {
    if size == 0
        || size > MAX
        || payload.len() > MAX
        || base.len() > MAX
        || codec > 2
        || codec != 2 && !base.is_empty()
        || codec == 2 && base.is_empty()
    {
        return Err(invalid());
    }
    if codec == 0 {
        return if payload.len() == size {
            Ok(payload.to_vec())
        } else {
            Err(invalid())
        };
    }
    let mut output = Vec::with_capacity(size);
    let mut offset = 0;
    while offset < payload.len() {
        let token = read_integer(payload, &mut offset)?;
        let length = (token >> 1)
            .checked_add(if token & 1 == 0 { 1 } else { 4 })
            .ok_or_else(invalid)?;
        let length = usize::try_from(length).map_err(|_| invalid())?;
        if length > size - output.len() {
            return Err(invalid());
        }
        if token & 1 == 0 {
            let end = offset.checked_add(length).ok_or_else(invalid)?;
            output.extend_from_slice(payload.get(offset..end).ok_or_else(invalid)?);
            offset = end;
        } else {
            let source =
                usize::try_from(read_integer(payload, &mut offset)?).map_err(|_| invalid())?;
            if source >= base.len() + output.len() {
                return Err(invalid());
            }
            for at in source..source + length {
                let byte = if at < base.len() {
                    base[at]
                } else {
                    output[at - base.len()]
                };
                output.push(byte);
            }
        }
    }
    if output.len() != size {
        return Err(invalid());
    }
    Ok(output)
}

fn slot(bytes: &[u8]) -> usize {
    let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    (value.wrapping_mul(2654435761) >> 19) as usize
}

fn literals(output: &mut Vec<u8>, bytes: &[u8]) {
    if !bytes.is_empty() {
        integer(output, ((bytes.len() - 1) as u64) * 2);
        output.extend_from_slice(bytes);
    }
}

fn integer(output: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        output.push(value as u8 | 128);
        value >>= 7;
    }
    output.push(value as u8);
}

fn read_integer(bytes: &[u8], offset: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..=63).step_by(7) {
        let byte = *bytes.get(*offset).ok_or_else(invalid)?;
        *offset += 1;
        if shift == 63 && byte > 1 || shift > 0 && byte == 0 {
            return Err(invalid());
        }
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
    }
    Err(invalid())
}

fn invalid() -> Error {
    Error::operation("history chunk checksum mismatch: invalid encoding")
}
