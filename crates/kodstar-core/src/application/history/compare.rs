// Purpose: 고정된 두 소스 상태의 파일·텍스트 변화 스트리밍.
// Pattern: Query.
// Usage: history diff가 체크포인트 ID를 결정해 호출.
// Related: crates/kodstar-core/src/infrastructure/history/checkpoints.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::Result,
    history::HistoryRow,
    results::{QueryRow, QuerySink},
};
use crate::infrastructure::{
    cache::paths::{encoding, path_from_bytes},
    history::{Repository, changes, checkpoints},
};
use rusqlite::OptionalExtension;

pub(super) fn run(
    repository: &Repository,
    head: i64,
    from: Option<&str>,
    to: Option<&str>,
    limit: usize,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<u64> {
    let connection = &repository.connection;
    let to = to
        .map(|s| checkpoints::resolve(connection, s))
        .transpose()?
        .unwrap_or(head);
    let from = if let Some(from) = from {
        Some(checkpoints::resolve(connection, from)?)
    } else {
        connection
            .query_row("SELECT parent FROM checkpoints WHERE id=?1", [to], |r| {
                r.get(0)
            })
            .optional()?
            .flatten()
    };
    changes::prepare(connection, from, to, cancel)?;
    let mut statement=connection.prepare("SELECT c.path,c.before_blob,c.after_blob,
        CASE WHEN x.hash IS NOT NULL THEN lower(hex(x.hash)) END,
        CASE WHEN y.hash IS NOT NULL THEN lower(hex(y.hash)) END,
        CASE WHEN c.before_blob IS NULL THEN NULL WHEN c.before_attributes&512 THEN 'symlink' ELSE 'file' END,
        CASE WHEN c.after_blob IS NULL THEN NULL WHEN c.after_attributes&512 THEN 'symlink' ELSE 'file' END,
        c.before_attributes&511,c.after_attributes&511 FROM snapshot_changes c
        LEFT JOIN blobs x ON x.id=c.before_blob LEFT JOIN blobs y ON y.id=c.after_blob ORDER BY c.path")?;
    let mut rows = statement.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let path = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        let before: Option<i64> = row.get(1)?;
        let after: Option<i64> = row.get(2)?;
        let change = if before.is_none() {
            "added"
        } else if after.is_none() {
            "deleted"
        } else {
            "modified"
        };
        sink.row(&QueryRow::History(HistoryRow::Change {
            path: path.to_string_lossy().into_owned(),
            path_encoding: encoding(&path),
            change: change.into(),
            before: row.get(3)?,
            after: row.get(4)?,
            before_kind: row.get(5)?,
            after_kind: row.get(6)?,
            before_mode: row.get(7)?,
            after_mode: row.get(8)?,
        }))?;
        count += 1;
        count += super::text::render(connection, &path, before, after, limit, cancel, sink)?;
    }
    Ok(count)
}
