// Purpose: 체크포인트·브랜치 이력의 행 단위 조회.
// Pattern: Query.
// Usage: history 사용 사례가 결과 sink에 전달.
// Related: crates/kodstar-core/src/application/history/mod.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::Result,
    history::HistoryRow,
    results::{QueryRow, QuerySink},
};
use crate::infrastructure::history::{Repository, checkpoints::display};

pub(super) fn checkpoints(
    repository: &Repository,
    head: i64,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<u64> {
    let mut statement=repository.connection.prepare("WITH RECURSIVE ancestry(id) AS (SELECT ?1 UNION SELECT c.parent FROM checkpoints c JOIN ancestry a ON a.id=c.id WHERE c.parent IS NOT NULL) SELECT c.id,c.parent,c.created_at,n.name,c.changed_files FROM checkpoints c JOIN ancestry a ON a.id=c.id LEFT JOIN names n ON n.checkpoint=c.id ORDER BY c.id DESC,n.name")?;
    let mut rows = statement.query([head])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        sink.row(&QueryRow::History(HistoryRow::Checkpoint {
            id: display(row.get(0)?),
            parent: row.get::<_, Option<i64>>(1)?.map(display),
            created_at: row.get(2)?,
            name: row.get(3)?,
            changed_files: row.get(4)?,
        }))?;
        count += 1;
    }
    Ok(count)
}

pub(super) fn branches(
    repository: &Repository,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
) -> Result<u64> {
    let mut statement=repository.connection.prepare("SELECT b.name,b.checkpoint,b.name=r.current_branch FROM branches b CROSS JOIN repository r ORDER BY b.name")?;
    let mut rows = statement.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        sink.row(&QueryRow::History(HistoryRow::Branch {
            name: row.get(0)?,
            checkpoint: display(row.get(1)?),
            current: row.get(2)?,
        }))?;
        count += 1;
    }
    Ok(count)
}
