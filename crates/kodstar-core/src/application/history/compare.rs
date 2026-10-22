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
    history::{Repository, checkpoints},
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
    if let Some(from) = from {
        checkpoints::materialize(connection, from)?;
    } else {
        connection.execute_batch("DROP TABLE IF EXISTS temp.snapshot_files; CREATE TEMP TABLE snapshot_files(path BLOB,blob INTEGER,mode INTEGER,kind TEXT);")?;
    }
    connection.execute_batch("DROP TABLE IF EXISTS temp.diff_before; CREATE TEMP TABLE diff_before AS SELECT * FROM snapshot_files; CREATE UNIQUE INDEX temp.diff_before_path ON diff_before(path);")?;
    checkpoints::materialize(connection, to)?;
    let mut statement=connection.prepare("SELECT a.path,a.blob,b.blob,x.hash,y.hash,a.kind,b.kind,a.mode,b.mode FROM diff_before a LEFT JOIN snapshot_files b ON a.path=b.path LEFT JOIN blobs x ON x.id=a.blob LEFT JOIN blobs y ON y.id=b.blob WHERE b.path IS NULL OR a.blob<>b.blob OR a.mode<>b.mode OR a.kind<>b.kind UNION ALL SELECT b.path,NULL,b.blob,NULL,y.hash,NULL,b.kind,NULL,b.mode FROM snapshot_files b JOIN blobs y ON y.id=b.blob WHERE NOT EXISTS(SELECT 1 FROM diff_before a WHERE a.path=b.path) ORDER BY 1")?;
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
