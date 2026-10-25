// Purpose: 부분 무효화에서 관계 블록을 한 번씩만 다시 작성.
// Pattern: Bounded Mutation.
// Usage: incremental resolver가 삭제할 논리 관계를 먼저 디스크에 선택.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs, crates/kodstar-core/src/infrastructure/index/invalidation.rs.

use rusqlite::{Connection, Result, params_from_iter};
use std::collections::BTreeMap;

pub(crate) fn partial(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE relation_pending_blocks(id INTEGER PRIMARY KEY);
        INSERT INTO relation_pending_blocks SELECT id FROM relation_blocks WHERE file_id IN (SELECT id FROM changed_files);")?;
    c.execute_batch("INSERT OR IGNORE INTO relation_pending_blocks
        SELECT r.block FROM affected_facts a CROSS JOIN relation_routes r ON r.kind=1 AND r.entity=a.id;")?;
    let mut writer = super::Writer::new(c)?;
    let mut prior = c.prepare(super::prior::INSERT)?;
    let slots = std::iter::repeat_n("?", 128).collect::<Vec<_>>().join(",");
    let mut selected = c.prepare(&format!(
        "SELECT id,
        (file_id IN (SELECT id FROM changed_files) OR id IN (SELECT id FROM affected_facts))
        AND NOT (language='go' AND relation='implements' AND expression LIKE '@go-interface:%')
        FROM facts WHERE id IN ({slots})"
    ))?;
    let mut next_block =
        c.prepare("SELECT id FROM relation_pending_blocks WHERE id>?1 ORDER BY id LIMIT 1")?;
    let mut after = 0;
    loop {
        use rusqlite::OptionalExtension;
        let block: Option<i64> = next_block.query_row([after], |r| r.get(0)).optional()?;
        let Some(block) = block else {
            break;
        };
        let (file, source, mut rows) = super::load(c, block)?;
        let original = rows.len();
        let ids = rows
            .iter()
            .map(|r| Some(r.fact))
            .chain(std::iter::repeat(None))
            .take(128);
        let remove = selected
            .query_map(params_from_iter(ids), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?))
            })?
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut retained = Vec::with_capacity(rows.len());
        for row in rows.drain(..) {
            if !*remove.get(&row.fact).ok_or_else(super::invalid)? {
                retained.push(row);
            } else {
                super::prior::remember(&mut prior, &row)?;
            }
        }
        if retained.len() != original {
            writer.store(block, file, source, &retained)?;
        }
        after = block;
    }
    c.execute_batch("DROP TABLE relation_pending_blocks;")
}
