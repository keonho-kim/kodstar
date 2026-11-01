// Purpose: 두 체크포인트 사이의 최종 경로 변화만 작업 수명의 표로 제공.
// Pattern: Incremental Reduction.
// Usage: 분석 discovery·history diff·영향 조회가 같은 변경 전후 계약을 소비.
// Related: crates/kodstar-core/src/infrastructure/history/changes/route.rs, crates/kodstar-core/src/infrastructure/history/checkpoints.rs.

mod route;
#[cfg(test)]
mod tests;

use crate::domain::{cancellation::Cancellation, diagnostics::Result};
use rusqlite::Connection;

/// Leaves one row per net change in `snapshot_changes`; no source bytes are read.
/// The caller owns the repository lock or a pinned history read transaction.
pub(crate) fn prepare(
    c: &Connection,
    from: Option<i64>,
    to: i64,
    cancel: &Cancellation,
) -> Result<u64> {
    c.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS snapshot_changes(
        path BLOB PRIMARY KEY,before_blob INTEGER,before_attributes INTEGER,
        after_blob INTEGER,after_attributes INTEGER) WITHOUT ROWID;
        DELETE FROM snapshot_changes;",
    )?;
    let outcome: Result<u64> = (|| {
        if route::prepare(c, from, to, cancel)? {
            fold(c, cancel)?;
        } else {
            compare(c, from, to)?;
        }
        cancel.check()?;
        c.execute(
            "DELETE FROM snapshot_changes WHERE before_blob IS after_blob
            AND before_attributes IS after_attributes",
            [],
        )?;
        Ok(c.query_row("SELECT count(*) FROM snapshot_changes", [], |r| r.get(0))?)
    })();
    let cleanup =
        c.execute_batch("DROP TABLE IF EXISTS change_route; DROP TABLE IF EXISTS change_before;");
    let count = outcome?;
    cleanup?;
    Ok(count)
}

fn fold(c: &Connection, cancel: &Cancellation) -> Result<()> {
    for (direction, order, before, after) in
        [(-1, "ASC", "", "before_"), (1, "DESC", "before_", "")]
    {
        let mut steps = c.prepare(&format!(
            "SELECT id FROM change_route WHERE direction=?1 ORDER BY sequence {order}"
        ))?;
        let mut rows = steps.query([direction])?;
        let mut merge = c.prepare(&format!("INSERT INTO snapshot_changes
            SELECT p.value,e.{before}blob,CASE WHEN e.{before}blob IS NOT NULL THEN e.{before}attributes END,
                e.{after}blob,CASE WHEN e.{after}blob IS NOT NULL THEN e.{after}attributes END
            FROM entry_rows e JOIN paths p ON p.id=e.path WHERE e.checkpoint=?1
            ON CONFLICT(path) DO UPDATE SET after_blob=excluded.after_blob,
                after_attributes=excluded.after_attributes"))?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            merge.execute([row.get::<_, i64>(0)?])?;
        }
    }
    Ok(())
}

fn compare(c: &Connection, from: Option<i64>, to: i64) -> Result<()> {
    if let Some(from) = from {
        super::checkpoints::materialize(c, from)?;
        c.execute_batch(
            "CREATE TEMP TABLE change_before AS SELECT * FROM snapshot_files;
            CREATE UNIQUE INDEX temp.change_before_path ON change_before(path);",
        )?;
    } else {
        c.execute_batch(
            "CREATE TEMP TABLE change_before(path BLOB PRIMARY KEY,blob INTEGER,
            mode INTEGER,kind TEXT) WITHOUT ROWID;",
        )?;
    }
    super::checkpoints::materialize(c, to)?;
    c.execute_batch("INSERT INTO snapshot_changes
        SELECT a.path,a.blob,a.mode|(CASE WHEN a.kind='symlink' THEN 512 ELSE 0 END),
            b.blob,CASE WHEN b.blob IS NOT NULL THEN b.mode|(CASE WHEN b.kind='symlink' THEN 512 ELSE 0 END) END
        FROM change_before a LEFT JOIN snapshot_files b ON b.path=a.path
        WHERE a.blob IS NOT b.blob OR a.mode IS NOT b.mode OR a.kind IS NOT b.kind;
        INSERT INTO snapshot_changes
        SELECT b.path,NULL,NULL,b.blob,b.mode|(CASE WHEN b.kind='symlink' THEN 512 ELSE 0 END)
        FROM snapshot_files b WHERE NOT EXISTS(SELECT 1 FROM change_before a WHERE a.path=b.path);")?;
    Ok(())
}
