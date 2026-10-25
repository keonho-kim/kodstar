// Purpose: 고정 버퍼로 공통 앞·뒤 구간을 비교해 완전한 변경 행 범위 선택.
// Pattern: Streaming Comparison.
// Usage: 텍스트 diff가 검증된 두 blob에서 표시 범위를 계산.
// Related: crates/kodstar-core/src/infrastructure/history/content/reader.rs, crates/kodstar-core/src/application/history/text.rs.

use crate::domain::{cancellation::Cancellation, diagnostics::Result};
use crate::infrastructure::history::content::{blobs::CHUNK, chunk_ranges, reader::Reader};
use rusqlite::Connection;
use std::ops::Range;

pub(super) fn changed(
    connection: &Connection,
    before: i64,
    after: i64,
    cancel: &Cancellation,
) -> Result<(Range<u64>, Range<u64>, u64)> {
    let mut a = Reader::verified(connection, before)?;
    let mut b = Reader::verified(connection, after)?;
    let mut left = [0; CHUNK];
    let mut right = [0; CHUNK];
    let common = chunk_ranges::prefix(connection, before, after, cancel)?;
    let mut offset = common.bytes;
    let mut start = common.last_newline.map_or(0, |at| at + 1);
    let mut line = common.lines + 1;
    while offset < a.size.min(b.size) {
        let count = (a.size.min(b.size) - offset).min(CHUNK as u64) as usize;
        a.read_at(offset, &mut left[..count], cancel)?;
        b.read_at(offset, &mut right[..count], cancel)?;
        let equal = left[..count]
            .iter()
            .zip(&right[..count])
            .take_while(|(a, b)| a == b)
            .count();
        if let Some(index) = memchr::memrchr(b'\n', &left[..equal]) {
            start = offset + index as u64 + 1;
            line += memchr::memchr_iter(b'\n', &left[..equal]).count() as u64;
        }
        offset += equal as u64;
        if equal < count {
            break;
        }
    }
    let common_suffix =
        chunk_ranges::suffix(connection, before, after, (a.size, b.size), start, cancel)?;
    let mut suffix = common_suffix.bytes;
    while suffix < (a.size - start).min(b.size - start) {
        let count = ((a.size - start).min(b.size - start) - suffix).min(CHUNK as u64) as usize;
        a.read_at(a.size - suffix - count as u64, &mut left[..count], cancel)?;
        b.read_at(b.size - suffix - count as u64, &mut right[..count], cancel)?;
        let equal = left[..count]
            .iter()
            .rev()
            .zip(right[..count].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        suffix += equal as u64;
        if equal < count {
            break;
        }
    }
    let mut adjustment = 0;
    let extra = suffix - common_suffix.bytes;
    let mut found_newline = false;
    while adjustment < extra {
        let count = (extra - adjustment).min(CHUNK as u64) as usize;
        a.read_at(a.size - suffix + adjustment, &mut left[..count], cancel)?;
        if let Some(index) = memchr::memchr(b'\n', &left[..count]) {
            adjustment += index as u64 + 1;
            found_newline = true;
            break;
        }
        adjustment += count as u64;
    }
    if !found_newline {
        adjustment = common_suffix
            .first_newline
            .map_or(suffix, |at| at - (a.size - suffix) + 1);
    }
    Ok((
        start..a.size - suffix + adjustment,
        start..b.size - suffix + adjustment,
        line,
    ))
}
