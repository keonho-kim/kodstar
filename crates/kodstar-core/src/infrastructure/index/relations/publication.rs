// Purpose: 해석 staging의 관계를 파일·source별 제한 블록으로 일괄 공개.
// Pattern: Streaming Publication.
// Usage: resolver flush가 같은 분석 transaction 안에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs, crates/kodstar-core/src/application/analysis/resolve/publication.rs.

use super::{Relation, merge::Blocks, prior::Prior};
use rusqlite::{Connection, Result};

pub(crate) fn publish(c: &Connection) -> Result<()> {
    c.execute_batch("INSERT OR IGNORE INTO relation_texts(value) SELECT rule FROM resolution_relationships UNION SELECT reason FROM resolution_relationships WHERE reason IS NOT NULL;")?;
    let mut select=c.prepare("SELECT f.file_id,r.fact_id,r.source_id,r.target_id,r.external_key,r.resolution,t.id,reason.id FROM resolution_relationships r JOIN facts f ON f.id=r.fact_id JOIN relation_texts t ON t.value=r.rule LEFT JOIN relation_texts reason ON reason.value=r.reason ORDER BY f.file_id,r.source_id,r.id")?;
    let mut input = select.query([])?;
    let mut prior = Prior::new(c)?;
    let mut blocks = Blocks::new(c)?;
    while let Some(row) = input.next()? {
        let group = (row.get::<_, i64>(0)?, row.get::<_, i64>(2)?);
        let mut value = Relation {
            id: 0,
            fact: row.get(1)?,
            source: row.get(2)?,
            target: row.get(3)?,
            external: row.get(4)?,
            state: Relation::state(row.get_ref(5)?.as_str()?)?,
            rule: row.get(6)?,
            reason: row.get(7)?,
        };
        value.id = prior.assign(&value)?;
        blocks.push(group.0, group.1, value)?;
    }
    blocks.flush()
}
