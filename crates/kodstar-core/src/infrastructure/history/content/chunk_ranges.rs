// Purpose: 검증된 공유 청크 주소와 줄 metadata로 같은 앞·뒤 구간 선택.
// Pattern: Storage Query.
// Usage: 텍스트 diff가 전체 내용 검증 후 재읽기를 줄임.
// Related: crates/kodstar-core/src/infrastructure/history/content/chunks.rs, crates/kodstar-core/src/application/history/text/ranges.rs.

use crate::domain::{cancellation::Cancellation, diagnostics::Result};
use rusqlite::{Connection, params};

pub(crate) struct Prefix {
    pub bytes: u64,
    pub lines: u64,
    pub last_newline: Option<u64>,
}

pub(crate) fn prefix(
    connection: &Connection,
    before: i64,
    after: i64,
    cancel: &Cancellation,
) -> Result<Prefix> {
    let mut statement = connection.prepare("SELECT a.start,a.chunk=b.chunk,d.size,d.lines,d.last_newline FROM chunk_refs a JOIN chunk_refs b ON b.blob=?2 AND b.part=a.part JOIN chunk_data d ON d.id=a.chunk WHERE a.blob=?1 ORDER BY a.part")?;
    let mut rows = statement.query(params![before, after])?;
    let mut prefix = Prefix {
        bytes: 0,
        lines: 0,
        last_newline: None,
    };
    while let Some(row) = rows.next()? {
        cancel.check()?;
        if !row.get::<_, bool>(1)? {
            break;
        }
        let start: u64 = row.get(0)?;
        let size: u64 = row.get(2)?;
        let lines: u64 = row.get(3)?;
        let last: i64 = row.get(4)?;
        if last >= 0 {
            prefix.last_newline = Some(start + last as u64);
        }
        prefix.bytes += size;
        prefix.lines += lines;
    }
    Ok(prefix)
}

pub(crate) struct Suffix {
    pub bytes: u64,
    pub first_newline: Option<u64>,
}

pub(crate) fn suffix(
    connection: &Connection,
    before: i64,
    after: i64,
    sizes: (u64, u64),
    start: u64,
    cancel: &Cancellation,
) -> Result<Suffix> {
    let mut suffix = Suffix {
        bytes: 0,
        first_newline: None,
    };
    let delta: i64 = connection.query_row("SELECT coalesce((SELECT max(part) FROM chunk_refs WHERE blob=?2),0)-coalesce((SELECT max(part) FROM chunk_refs WHERE blob=?1),0)",params![before,after],|r|r.get(0))?;
    let mut statement=connection.prepare("SELECT a.chunk=b.chunk,d.size,a.start,d.first_newline FROM chunk_refs a JOIN chunk_refs b ON b.blob=?2 AND b.part=a.part+?3 JOIN chunk_data d ON d.id=a.chunk WHERE a.blob=?1 AND a.start>=?4 ORDER BY a.part DESC")?;
    let mut rows = statement.query(params![before, after, delta, start])?;
    let maximum = (sizes.0 - start).min(sizes.1 - start);
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let size: u64 = row.get(1)?;
        if !row.get::<_, bool>(0)? || size > maximum - suffix.bytes {
            break;
        }
        suffix.bytes += size;
        let first: i64 = row.get(3)?;
        if first >= 0 {
            let offset: u64 = row.get(2)?;
            suffix.first_newline = Some(offset + first as u64);
        }
    }
    Ok(suffix)
}
