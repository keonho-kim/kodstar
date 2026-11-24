// Purpose: 제한된 변경 창을 행 정렬하고 동일 행을 제외해 원본 행 번호로 출력.
// Pattern: Bounded Transformation.
// Usage: history/text.rs가 검증된 두 텍스트 범위에 적용.
// Related: crates/kodstar-core/src/application/history/text.rs, crates/kodstar-core/src/application/history/text/lines.rs.

use super::lines::Lines;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    results::QuerySink,
};
use crate::infrastructure::history::content::{blobs::CHUNK, reader::Reader};
use rusqlite::Connection;
use similar::{Algorithm, DiffTag, TextDiff};
use std::{ops::Range, path::Path};

// Both limits apply to the combined window. They bound the library's input and
// worst-case edit work independently of the file display acceptance limit.
const MAX_BYTES: u64 = 256 * 1024;
const MAX_LINES: usize = 4096;

pub(super) fn render(
    connection: &Connection,
    path: &Path,
    windows: [(i64, Range<u64>); 2],
    line: u64,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<Option<u64>> {
    if windows.iter().map(|(_, r)| r.end - r.start).sum::<u64>() > MAX_BYTES {
        return Ok(None);
    }
    let [left, right] = windows;
    let before = read(connection, left, cancel)?;
    let after = read(connection, right, cancel)?;
    if line_count(&before) + line_count(&after) > MAX_LINES {
        return Ok(None);
    }
    cancel.check()?;
    let span = tracing::info_span!("diff_align", bytes = before.len() + after.len());
    let _entered = span.enter();
    // The history contract counts LF only; the library's default tokenizer also
    // splits bare CR, which would change both line numbers and the work budget.
    let before_lines: Vec<_> = before.split_inclusive('\n').collect();
    let after_lines: Vec<_> = after.split_inclusive('\n').collect();
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_slices(&before_lines, &after_lines);
    cancel.check()?;
    let mut count = 0;
    for (old, side) in [(true, "before"), (false, "after")] {
        let mut output = Lines {
            path,
            side,
            line,
            cancel,
            sink,
        };
        for op in diff.ops().iter().filter(|op| op.tag() != DiffTag::Equal) {
            let range = if old { op.old_range() } else { op.new_range() };
            for index in range {
                cancel.check()?;
                output.line = line + index as u64;
                let text = if old {
                    diff.old_slice(index)
                } else {
                    diff.new_slice(index)
                }
                .ok_or_else(|| Error::operation("invalid aligned diff line"))?;
                output.emit(text.to_owned())?;
                count += 1;
            }
        }
    }
    Ok(Some(count))
}

fn line_count(text: &str) -> usize {
    memchr::memchr_iter(b'\n', text.as_bytes()).count()
        + usize::from(!text.is_empty() && !text.ends_with('\n'))
}

fn read(
    connection: &Connection,
    (id, range): (i64, Range<u64>),
    cancel: &Cancellation,
) -> Result<String> {
    let mut reader = Reader::verified(connection, id)?;
    let mut bytes = vec![0; (range.end - range.start) as usize];
    let mut offset = range.start;
    for chunk in bytes.chunks_mut(CHUNK) {
        let read = reader.read_at(offset, chunk, cancel)?;
        if read != chunk.len() {
            return Err(Error::operation("text diff range exceeds stored source"));
        }
        offset += read as u64;
    }
    String::from_utf8(bytes).map_err(|e| Error::caused_by("invalid text diff boundary", e))
}
