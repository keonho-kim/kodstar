// Purpose: 사실 교체·정의 삭제가 같은 관계 블록을 반복 작성하지 않도록 일괄 변경.
// Pattern: Bounded Mutation.
// Usage: 분석 writer가 사실·정의의 FK cascade 전에 호출.
// Related: crates/kodstar-core/src/infrastructure/index/write.rs, crates/kodstar-core/src/infrastructure/index/relations/writer.rs.

use super::Writer;
use rusqlite::{Connection, OptionalExtension, Result, params};
use std::collections::BTreeSet;

pub(crate) fn clear_bodies(c: &Connection, file: i64) -> Result<()> {
    let mut after = 0;
    let mut writer = Writer::new(c)?;
    let mut keep = c.prepare("SELECT language='go' AND relation='implements' AND expression LIKE '@go-interface:%' FROM facts WHERE id=?1")?;
    loop {
        let block: Option<i64> = c
            .query_row(
                "SELECT id FROM relation_blocks WHERE file_id=?1 AND id>?2 ORDER BY id LIMIT 1",
                params![file, after],
                |r| r.get(0),
            )
            .optional()?;
        let Some(block) = block else {
            break;
        };
        let (_, source, rows) = super::load(c, block)?;
        let original = rows.len();
        let mut retained = Vec::with_capacity(original);
        for row in rows {
            if keep.query_row([row.fact], |r| r.get::<_, bool>(0))? {
                retained.push(row);
            }
        }
        if retained.len() != original {
            writer.store(block, file, source, &retained)?;
        }
        after = block;
    }
    Ok(())
}

pub(crate) fn remove_targets(c: &Connection, file: Option<i64>) -> Result<()> {
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM symbols s JOIN files f ON f.id=s.file_id
        CROSS JOIN relation_routes r ON r.kind=3 AND r.entity=s.id
        WHERE (?1 IS NOT NULL AND f.id=?1) OR (?1 IS NULL AND f.path NOT IN (SELECT path FROM seen_files)))", [file], |r| r.get(0))?;
    if !exists {
        return Ok(());
    }
    c.execute_batch(
        "CREATE TEMP TABLE relation_removed_targets(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE relation_target_blocks(id INTEGER PRIMARY KEY);",
    )?;
    c.execute("INSERT INTO relation_removed_targets SELECT s.id FROM symbols s JOIN files f ON f.id=s.file_id
        WHERE (?1 IS NOT NULL AND f.id=?1) OR (?1 IS NULL AND f.path NOT IN (SELECT path FROM seen_files))", [file])?;
    c.execute_batch("INSERT OR IGNORE INTO relation_target_blocks
        SELECT r.block FROM relation_removed_targets t CROSS JOIN relation_routes r ON r.kind=3 AND r.entity=t.id;")?;
    let mut after = 0;
    let mut writer = Writer::new(c)?;
    let mut targets = c.prepare("SELECT r.entity FROM relation_routes r JOIN relation_removed_targets t ON t.id=r.entity WHERE r.kind=3 AND r.block=?1")?;
    loop {
        let block: Option<i64> = c
            .query_row(
                "SELECT id FROM relation_target_blocks WHERE id>?1 ORDER BY id LIMIT 1",
                [after],
                |r| r.get(0),
            )
            .optional()?;
        let Some(block) = block else {
            break;
        };
        // At most one block's target routes are retained in memory.
        let removed = targets
            .query_map([block], |r| r.get::<_, i64>(0))?
            .collect::<Result<BTreeSet<_>>>()?;
        let (file, source, mut rows) = super::load(c, block)?;
        rows.retain(|r| r.target.is_none_or(|t| !removed.contains(&t)));
        writer.store(block, file, source, &rows)?;
        after = block;
    }
    c.execute_batch("DROP TABLE relation_target_blocks; DROP TABLE relation_removed_targets;")
}
