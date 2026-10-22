// Purpose: 제한된 텍스트 diff의 검증·변경 범위·스트리밍 표시 조정.
// Pattern: Output Transformation.
// Usage: 파일 diff가 내용 해시 비교 후 호출.
// Related: crates/kodstar-core/src/application/history/compare.rs, crates/kodstar-core/src/application/history/text/ranges.rs.

mod lines;
mod ranges;
mod validation;

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::Result,
    history::HistoryRow,
    results::{QueryRow, QuerySink},
};
use crate::infrastructure::cache::paths::encoding;
use rusqlite::Connection;
use std::path::Path;
use validation::Content;

pub(super) fn render(
    connection: &Connection,
    path: &Path,
    before: Option<i64>,
    after: Option<i64>,
    max: usize,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<u64> {
    if before == after {
        return Ok(0);
    }
    let before = validation::read(connection, before, max, cancel)?;
    let after = validation::read(connection, after, max, cancel)?;
    let (left, right, line) = match (before, after) {
        (Content::Text { id: a, .. }, Content::Text { id: b, .. }) => {
            ranges::changed(connection, a, b, cancel)?
        }
        _ => (0..u64::MAX, 0..u64::MAX, 1),
    };
    let mut count = 0;
    for (side, content, range) in [("before", before, left), ("after", after, right)] {
        match content {
            Content::Absent => {}
            Content::Omitted(reason) => {
                sink.row(&QueryRow::History(HistoryRow::DiffOmitted {
                    path: path.to_string_lossy().into_owned(),
                    path_encoding: encoding(path),
                    side: side.into(),
                    reason: reason.into(),
                }))?;
                count += 1;
            }
            Content::Text { id, size } => {
                count += lines::Lines {
                    path,
                    side,
                    line,
                    cancel,
                    sink,
                }
                .render(
                    connection,
                    id,
                    range.start.min(size)..range.end.min(size),
                )?;
            }
        }
    }
    Ok(count)
}
