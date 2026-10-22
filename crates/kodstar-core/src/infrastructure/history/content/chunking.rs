// Purpose: 큰 파일의 삽입 뒤에도 같은 바이트 경계를 찾는 Gear 기반 분할.
// Pattern: Bounded Stream.
// Usage: blob 저장이 제한된 BufRead 입력에서 한 청크씩 소비.
// Related: crates/kodstar-core/src/infrastructure/history/content/blobs.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use std::io::BufRead;

pub(in crate::infrastructure::history) const MIN: usize = 2048;
pub(in crate::infrastructure::history) const AVERAGE: usize = 8192;
pub(in crate::infrastructure::history) const MAX: usize = 32768;
const GEAR: [u64; 256] = table();

const fn table() -> [u64; 256] {
    let mut output = [0; 256];
    let mut value = 0x6b6f647374617201u64;
    let mut index = 0;
    while index < 256 {
        value = value.wrapping_add(0x9e3779b97f4a7c15);
        let mut mixed = value;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
        output[index] = mixed ^ (mixed >> 31);
        index += 1;
    }
    output
}

pub(in crate::infrastructure::history) fn read(
    input: &mut impl BufRead,
    output: &mut [u8],
    cancel: &Cancellation,
) -> Result<usize> {
    if output.len() < MAX {
        return Err(Error::operation(
            "history chunk buffer is smaller than its maximum",
        ));
    }
    let mut used = 0;
    let mut hash = 0u64;
    loop {
        cancel.check()?;
        let available = match input.fill_buf() {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::caused_by("read history chunks", e)),
        };
        if available.is_empty() {
            return Ok(used);
        }
        let mut consumed = 0;
        let mut boundary = false;
        for byte in available {
            output[used] = *byte;
            used += 1;
            consumed += 1;
            hash = hash.wrapping_shl(1).wrapping_add(GEAR[*byte as usize]);
            let mask = if used < AVERAGE { 16383 } else { 4095 };
            if used == MAX || used >= MIN && hash & mask == 0 {
                boundary = true;
                break;
            }
        }
        input.consume(consumed);
        if boundary {
            return Ok(used);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};
    fn split(bytes: &[u8], capacity: usize) -> Result<Vec<Vec<u8>>> {
        let mut input = BufReader::with_capacity(capacity, Cursor::new(bytes));
        let mut buffer = [0; MAX];
        let mut result = Vec::new();
        loop {
            let size = read(&mut input, &mut buffer, &Cancellation::default())?;
            if size == 0 {
                break;
            }
            result.push(buffer[..size].to_vec());
        }
        Ok(result)
    }
    #[test]
    fn boundaries_ignore_input_reads_and_resynchronize_after_insertions() -> Result<()> {
        let mut state = 123456u64;
        let bytes: Vec<u8> = (0..1024 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let original = split(&bytes, 65536)?;
        assert_eq!(original, split(&bytes, 17)?);
        assert_eq!(original.concat(), bytes);
        assert!(original.iter().all(|c| c.len() <= MAX));
        let changed = split(&[b"insert".as_slice(), &bytes].concat(), 4096)?;
        let reused: usize = changed
            .iter()
            .filter(|c| original.contains(c))
            .map(Vec::len)
            .sum();
        assert!(reused > bytes.len() * 9 / 10);
        Ok(())
    }
}
