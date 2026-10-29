// Purpose: 가까운 체크포인트의 양방향 변경 적용과 먼 시점의 전체 재구성.
// Pattern: Bounded Replay.
// Usage: snapshot·diff·checkout이 동일한 파일 구성 계약을 사용.
// Related: crates/kodstar-core/src/infrastructure/history/checkpoints.rs, crates/kodstar-core/src/infrastructure/history/metadata.sql.

use crate::domain::diagnostics::{Error, Result};
use rusqlite::{Connection, params};

#[cfg(test)]
mod tests;

pub(crate) fn run(c: &Connection, id: i64) -> Result<()> {
    c.execute_batch("DROP TABLE IF EXISTS temp.snapshot_files; CREATE TEMP TABLE snapshot_files(path BLOB PRIMARY KEY,blob INTEGER,mode INTEGER,kind TEXT) WITHOUT ROWID;")?;
    let head = super::head(c)?;
    if head == Some(id) {
        c.execute_batch("INSERT INTO snapshot_files SELECT path,blob,mode,kind FROM working;")?;
        return Ok(());
    }
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS manifest_route(id INTEGER PRIMARY KEY,direction INTEGER,sequence INTEGER); DELETE FROM manifest_route;")?;
    let replay = nearby(c, head, id);
    let cleanup = c.execute_batch("DROP TABLE manifest_route;");
    let replay = replay?;
    cleanup?;
    if !replay {
        c.execute("WITH RECURSIVE ancestry(id) AS (SELECT ?1 UNION SELECT c.parent FROM checkpoints c JOIN ancestry a ON c.id=a.id WHERE c.parent IS NOT NULL), latest AS (SELECT e.path,max(e.checkpoint) checkpoint FROM entries e JOIN ancestry a ON a.id=e.checkpoint GROUP BY e.path) INSERT INTO snapshot_files SELECT e.path,e.blob,e.mode,e.kind FROM entries e JOIN latest l ON l.path=e.path AND l.checkpoint=e.checkpoint WHERE e.blob IS NOT NULL", [id])?;
    }
    Ok(())
}

fn nearby(c: &Connection, head: Option<i64>, target: i64) -> Result<bool> {
    let (mut left, mut right) = (head, Some(target));
    let mut parent = c.prepare("SELECT parent FROM checkpoints WHERE id=?1")?;
    let mut route = c.prepare("INSERT INTO manifest_route VALUES(?1,?2,?3)")?;
    for step in 0..64 {
        if left == right {
            break;
        }
        let reverse = left > right;
        let current = if reverse { left } else { right }
            .ok_or_else(|| Error::operation("invalid history replay ancestry"))?;
        let next: Option<i64> = parent.query_row([current], |r| r.get(0))?;
        if next.is_some_and(|n| n >= current) {
            return Err(Error::operation("invalid history replay ancestry"));
        }
        route.execute(params![current, if reverse { -1 } else { 1 }, step])?;
        if reverse {
            left = next;
        } else {
            right = next;
        }
    }
    if left != right {
        return Ok(false);
    }
    if head.is_some() {
        c.execute_batch("INSERT INTO snapshot_files SELECT path,blob,mode,kind FROM working;")?;
    }
    let mut clear = c.prepare("DELETE FROM snapshot_files WHERE path IN (SELECT p.value FROM entry_rows e JOIN paths p ON p.id=e.path WHERE e.checkpoint=?1)")?;
    for (direction, order, blob, attributes) in [
        (-1, "ASC", "before_blob", "before_attributes"),
        (1, "DESC", "blob", "attributes"),
    ] {
        let mut insert = c.prepare(&format!("INSERT INTO snapshot_files SELECT p.value,e.{blob},e.{attributes}&511,CASE WHEN e.{attributes}&512 THEN 'symlink' ELSE 'file' END FROM entry_rows e JOIN paths p ON p.id=e.path WHERE e.checkpoint=?1 AND e.{blob} IS NOT NULL"))?;
        let mut checkpoints = c.prepare(&format!(
            "SELECT id FROM manifest_route WHERE direction=?1 ORDER BY sequence {order}"
        ))?;
        let mut rows = checkpoints.query([direction])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            clear.execute([id])?;
            insert.execute([id])?;
        }
    }
    Ok(true)
}
