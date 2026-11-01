// Purpose: 새 해석 결과에 남지 않은 이전 관계만 블록별로 회수.
// Pattern: Delta Publication.
// Usage: publication이 동일 관계를 소비한 뒤 새 관계를 병합하기 전에 호출.
// Related: crates/kodstar-core/src/infrastructure/index/relations/prior.rs, crates/kodstar-core/src/infrastructure/index/relations/publication.rs.

use rusqlite::{Connection, OptionalExtension, Result, params_from_iter};
use std::collections::BTreeSet;

pub(super) fn remove_stale(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE relation_stale_blocks(id INTEGER PRIMARY KEY);
        INSERT OR IGNORE INTO relation_stale_blocks
        SELECT r.block FROM relation_prior p CROSS JOIN relation_id_ranges r
        WHERE r.first=(SELECT first FROM relation_id_ranges WHERE first<=p.id ORDER BY first DESC LIMIT 1)
            AND r.last>=p.id;")?;
    let mut next =
        c.prepare("SELECT id FROM relation_stale_blocks WHERE id>?1 ORDER BY id LIMIT 1")?;
    let slots = std::iter::repeat_n("?", 128).collect::<Vec<_>>().join(",");
    let mut stale = c.prepare(&format!(
        "SELECT id FROM relation_prior WHERE id IN ({slots})"
    ))?;
    let mut writer = super::Writer::new(c)?;
    let mut after = 0;
    while let Some(block) = next.query_row([after], |r| r.get::<_, i64>(0)).optional()? {
        let (file, source, mut rows) = super::load(c, block)?;
        let ids = rows
            .iter()
            .map(|r| Some(r.id))
            .chain(std::iter::repeat(None))
            .take(128);
        let removed: BTreeSet<i64> = stale
            .query_map(params_from_iter(ids), |r| r.get(0))?
            .collect::<Result<_>>()?;
        rows.retain(|r| !removed.contains(&r.id));
        writer.store(block, file, source, &rows)?;
        after = block;
    }
    drop(next);
    drop(stale);
    drop(writer);
    c.execute_batch("DROP TABLE relation_stale_blocks;")
}
