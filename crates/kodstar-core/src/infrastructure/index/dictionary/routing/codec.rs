// Purpose: 고정 128개 ID 페이지의 블록 run-length를 제한적으로 검증·복원.
// Pattern: Bounded Codec.
// Usage: routing writer·정확 ID 조회·독립 행 view가 공유.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/routing/mod.rs.

use rusqlite::{Error, Result};
pub(super) const WIDTH: usize = 128;
fn invalid() -> Error {
    Error::ModuleError("invalid dictionary routing page".into())
}

fn push(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn number(bytes: &mut &[u8]) -> Result<u64> {
    let mut n = 0;
    for shift in (0..=63).step_by(7) {
        let (b, rest) = bytes.split_first().ok_or_else(invalid)?;
        if shift == 63 && *b > 1 || shift > 0 && *b == 0 {
            return Err(invalid());
        }
        n |= u64::from(b & 127) << shift;
        *bytes = rest;
        if b & 128 == 0 {
            return Ok(n);
        }
    }
    Err(invalid())
}

pub(super) fn encode(page: &[i64; WIDTH]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < WIDTH {
        let value = page[at];
        let mut end = at + 1;
        while end < WIDTH && page[end] == value {
            end += 1;
        }
        push(&mut out, (end - at) as u64);
        push(&mut out, value as u64);
        at = end;
    }
    out
}

pub(super) fn visit(mut bytes: &[u8], mut f: impl FnMut(usize, usize, i64)) -> Result<()> {
    if bytes.len() > WIDTH * 12 {
        return Err(invalid());
    }
    let mut at = 0;
    while at < WIDTH {
        let count = usize::try_from(number(&mut bytes)?).map_err(|_| invalid())?;
        let block = i64::try_from(number(&mut bytes)?).map_err(|_| invalid())?;
        let end = at
            .checked_add(count)
            .filter(|end| count > 0 && *end <= WIDTH)
            .ok_or_else(invalid)?;
        f(at, end, block);
        at = end;
    }
    if !bytes.is_empty() {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn decode(bytes: &[u8]) -> Result<[i64; WIDTH]> {
    let mut page = [0; WIDTH];
    visit(bytes, |a, b, n| page[a..b].fill(n))?;
    Ok(page)
}

pub(super) fn at(bytes: &[u8], slot: usize) -> Result<i64> {
    if slot >= WIDTH {
        return Err(invalid());
    }
    let mut value = 0;
    visit(bytes, |a, b, n| {
        if (a..b).contains(&slot) {
            value = n;
        }
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gaps_fragmentation_and_large_ids_preserve_every_slot() -> Result<()> {
        let mut page = [0; WIDTH];
        page[..32].fill(7);
        page[33] = i64::MAX;
        page[127] = 19;
        let bytes = encode(&page);
        assert_eq!(decode(&bytes)?, page);
        for (i, n) in page.iter().enumerate() {
            assert_eq!(at(&bytes, i)?, *n);
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(decode(&[0, 1]).is_err());
        assert!(decode(&[129, 1, 0]).is_err());
        Ok(())
    }
}
