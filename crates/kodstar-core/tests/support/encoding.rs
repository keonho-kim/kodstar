// Purpose: 새 이력 표현을 독립적으로 복원해 이전 형식 fixture를 생성.
// Pattern: Reference Decoder.
// Usage: 이력 전환 통합 테스트가 소유 DB 연결에 등록.
// Related: scripts/benchmarks/content_codec.py, crates/kodstar-core/tests/cases/history_upgrade.rs.

use rusqlite::{Connection, functions::FunctionFlags};

fn decode(payload: &[u8], codec: u8, size: usize, base: &[u8]) -> rusqlite::Result<Vec<u8>> {
    if size == 0
        || size > 65536
        || payload.len() > 65536
        || base.len() > 65536
        || codec > 2
        || (codec == 2) != !base.is_empty()
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if codec == 0 {
        return if payload.len() == size {
            Ok(payload.to_vec())
        } else {
            Err(rusqlite::Error::InvalidQuery)
        };
    }
    let mut output = Vec::new();
    let mut position = 0;
    fn number(bytes: &[u8], at: &mut usize) -> rusqlite::Result<u64> {
        let mut value = 0;
        for shift in (0..=63).step_by(7) {
            let byte = *bytes.get(*at).ok_or(rusqlite::Error::InvalidQuery)?;
            *at += 1;
            if shift == 63 && byte > 1 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            value |= u64::from(byte & 127) << shift;
            if byte < 128 {
                return Ok(value);
            }
        }
        Err(rusqlite::Error::InvalidQuery)
    }
    while position < payload.len() {
        let token = number(payload, &mut position)?;
        let length = (token >> 1) + (if token & 1 == 1 { 4 } else { 1 });
        if length > (size - output.len()) as u64 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if token & 1 == 1 {
            let start = number(payload, &mut position)?;
            if start >= (base.len() + output.len()) as u64 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            for at in start as usize..start as usize + length as usize {
                let byte = if at < base.len() {
                    base[at]
                } else {
                    output[at - base.len()]
                };
                output.push(byte);
            }
        } else {
            let end = position + length as usize;
            output.extend_from_slice(
                payload
                    .get(position..end)
                    .ok_or(rusqlite::Error::InvalidQuery)?,
            );
            position = end;
        }
    }
    if output.len() != size {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(output)
}

pub fn register(connection: &Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "kodstar_content",
        6,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |c| {
            let base = match c.get_raw(3) {
                rusqlite::types::ValueRef::Null => Vec::new(),
                value => decode(value.as_blob()?, c.get(4)?, c.get(5)?, &[])?,
            };
            decode(c.get_raw(0).as_blob()?, c.get(1)?, c.get(2)?, &base)
        },
    )
}
