// Purpose: 작은 변경은 분기 계보를 따라가고 전체 대조보다 비싼 경로는 중단.
// Pattern: Cost Bounded Traversal.
// Usage: history changes가 경로별 차이를 합성하기 전에 호출.
// Related: crates/kodstar-core/src/infrastructure/history/changes/mod.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::{Connection, params};

pub(super) fn prepare(
    c: &Connection,
    from: Option<i64>,
    to: i64,
    cancel: &Cancellation,
) -> Result<bool> {
    c.execute_batch(
        "CREATE TEMP TABLE change_route(id INTEGER PRIMARY KEY,
        direction INTEGER NOT NULL,sequence INTEGER NOT NULL);",
    )?;
    let mut query =
        c.prepare("SELECT parent,changed_files,tracked_files FROM checkpoints WHERE id=?1")?;
    let read = |r: &rusqlite::Row<'_>| {
        Ok((
            r.get::<_, Option<i64>>(0)?,
            r.get::<_, u64>(1)?,
            r.get::<_, u64>(2)?,
        ))
    };
    let (_, _, target_files) = query.query_row([to], read)?;
    let previous_files = from
        .map(|id| query.query_row([id], read))
        .transpose()?
        .map_or(0, |v| v.2);
    let budget = previous_files
        .saturating_add(target_files)
        .saturating_add(64);
    let (mut left, mut right, mut work) = (from, Some(to), 0u64);
    let mut insert = c.prepare("INSERT INTO change_route VALUES(?1,?2,?3)")?;
    for step in 0..4096 {
        cancel.check()?;
        if left == right {
            return Ok(true);
        }
        let reverse = left > right;
        let id = if reverse { left } else { right }
            .ok_or_else(|| Error::operation("invalid checkpoint ancestry"))?;
        let (parent, changed, _) = query.query_row([id], read)?;
        if parent.is_some_and(|parent| parent >= id) {
            return Err(Error::operation("invalid checkpoint ancestry order"));
        }
        work = work.saturating_add(changed).saturating_add(1);
        if work > budget {
            return Ok(false);
        }
        insert.execute(params![id, if reverse { -1 } else { 1 }, step])?;
        if reverse {
            left = parent;
        } else {
            right = parent;
        }
    }
    Ok(left == right)
}
