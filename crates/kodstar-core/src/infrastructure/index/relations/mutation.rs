// Purpose: 사실 교체·정의 삭제가 같은 관계 블록을 반복 작성하지 않도록 일괄 변경.
// Pattern: Bounded Mutation.
// Usage: 분석 writer가 사실·정의의 FK cascade 전에 호출.
// Related: crates/kodstar-core/src/infrastructure/index/write.rs, crates/kodstar-core/src/infrastructure/index/relations/writer.rs.

use super::Writer;
use rusqlite::{Connection, OptionalExtension, Result, params_from_iter};
use std::collections::BTreeSet;

pub(crate) fn clear_all(c: &Connection) -> Result<()> {
    if c.is_autocommit() {
        return Err(rusqlite::Error::ModuleError(
            "full relationship reset requires an owned transaction".into(),
        ));
    }
    // No reader can observe these intermediate states outside the owned transaction.
    // Empty routing makes block cascades skip per-key page maintenance.
    c.execute_batch(
        "DELETE FROM relation_route_pages;
        DELETE FROM relation_id_ranges; DELETE FROM relation_blocks;",
    )
}

pub(crate) fn clear_bodies(c: &Connection, file: i64, exact: bool) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE body_relation_blocks(id INTEGER PRIMARY KEY);")?;
    if exact {
        c.execute_batch("CREATE TEMP TABLE body_removed_facts(id INTEGER PRIMARY KEY);")?;
        c.execute(
            "INSERT INTO body_removed_facts SELECT fact_id FROM body_original WHERE file_id=?1",
            [file],
        )?;
        c.execute_batch(
            "INSERT OR IGNORE INTO body_relation_blocks SELECT r.block FROM body_removed_facts f
            CROSS JOIN relation_routes r ON r.kind=1 AND r.entity=f.id;",
        )?;
    } else {
        c.execute(
            "INSERT INTO body_relation_blocks SELECT id FROM relation_blocks WHERE file_id=?1",
            [file],
        )?;
    }
    let slots = std::iter::repeat_n("?", 128).collect::<Vec<_>>().join(",");
    let selected = if exact {
        format!("SELECT id FROM body_removed_facts WHERE id IN ({slots})")
    } else {
        format!(
            "SELECT id FROM facts WHERE id IN ({slots}) AND relation IN ('calls','references','dispatch_candidate')"
        )
    };
    let mut selected = c.prepare(&selected)?;
    let mut blocks =
        c.prepare("SELECT id FROM body_relation_blocks WHERE id>?1 ORDER BY id LIMIT 1")?;
    let mut after = 0;
    let mut writer = Writer::new(c)?;
    while let Some(block) = blocks
        .query_row([after], |r| r.get::<_, i64>(0))
        .optional()?
    {
        let (_, source, mut rows) = super::load(c, block)?;
        let original = rows.len();
        let ids = rows
            .iter()
            .map(|r| Some(r.fact))
            .chain(std::iter::repeat(None))
            .take(128);
        let removed: BTreeSet<i64> = selected
            .query_map(params_from_iter(ids), |r| r.get(0))?
            .collect::<Result<_>>()?;
        rows.retain(|r| !removed.contains(&r.fact));
        if rows.len() != original {
            writer.store(block, file, source, &rows)?;
        }
        after = block;
    }
    drop(selected);
    drop(blocks);
    drop(writer);
    c.execute_batch("DROP TABLE body_relation_blocks; DROP TABLE IF EXISTS body_removed_facts;")
}

pub(crate) fn remove_targets(c: &Connection, file: Option<i64>) -> Result<()> {
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM symbols s JOIN files f ON f.id=s.file_id
        CROSS JOIN relation_routes r ON r.kind=3 AND r.entity=s.id
        WHERE (?1 IS NOT NULL AND f.id=?1) OR (?1 IS NULL AND f.id IN (SELECT id FROM deleted_files)))", [file], |r| r.get(0))?;
    if !exists {
        return Ok(());
    }
    c.execute_batch(
        "CREATE TEMP TABLE relation_removed_targets(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE relation_target_blocks(id INTEGER PRIMARY KEY);",
    )?;
    c.execute("INSERT INTO relation_removed_targets SELECT s.id FROM symbols s JOIN files f ON f.id=s.file_id
        WHERE (?1 IS NOT NULL AND f.id=?1) OR (?1 IS NULL AND f.id IN (SELECT id FROM deleted_files))", [file])?;
    c.execute_batch("INSERT OR IGNORE INTO relation_target_blocks
        SELECT r.block FROM relation_removed_targets t CROSS JOIN relation_routes r ON r.kind=3 AND r.entity=t.id;")?;
    let mut after = 0;
    let mut writer = Writer::new(c)?;
    let mut targets = c.prepare("SELECT json_extract(r.value,'$[1]') FROM relation_blocks b,json_each(kodstar_relation_pairs(b.routes)) r JOIN relation_removed_targets t ON t.id=json_extract(r.value,'$[1]') WHERE b.id=?1 AND json_extract(r.value,'$[0]')=3")?;
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
