// Purpose: 명시적 형식 전환에서만 형식 4의 literal/copy 청크를 복원.
// Pattern: Migration Decoder.
// Usage: maintenance/compressed가 보존 원문을 검증한 뒤 Zstd로 다시 인코딩.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/compressed.rs.

use super::super::content::encoding::MAX;
use crate::domain::diagnostics::{Error, Result};

pub(super) fn decode(payload: &[u8], codec: u8, size: usize, base: &[u8]) -> Result<Vec<u8>> {
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
