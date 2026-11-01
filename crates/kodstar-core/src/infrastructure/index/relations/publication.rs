// Purpose: 같은 관계는 유지하고 실제 차이만 파일·source별 제한 블록에 공개.
// Pattern: Differential Streaming Publication.
// Usage: resolver flush가 같은 분석 transaction 안에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/relations/reconcile.rs, crates/kodstar-core/src/infrastructure/index/relations/prior.rs.

use super::{Relation, merge::Blocks, prior::Prior};
use rusqlite::{Connection, Result, Row, params};

const COLUMNS: &str =
    "f.file_id,r.fact_id,r.source_id,r.target_id,r.external_key,r.resolution,t.id,reason.id";
const SOURCE: &str = "resolution_relationships r JOIN facts f ON f.id=r.fact_id
    JOIN relation_texts t ON t.value=r.rule LEFT JOIN relation_texts reason ON reason.value=r.reason";

pub(crate) fn publish(c: &Connection) -> Result<()> {
    c.execute_batch("INSERT OR IGNORE INTO relation_texts(value) SELECT rule FROM resolution_relationships UNION SELECT reason FROM resolution_relationships WHERE reason IS NOT NULL;")?;
    let mut prior = Prior::new(c)?;
    let differential = prior.active();
    if differential {
        c.execute_batch("CREATE TEMP TABLE relation_assignments(stage INTEGER PRIMARY KEY,id INTEGER NOT NULL);")?;
    }
    let mut assignments = differential
        .then(|| c.prepare("INSERT INTO relation_assignments VALUES(?1,?2)"))
        .transpose()?;
    let mut select = c.prepare(&format!(
        "SELECT {COLUMNS},r.id FROM {SOURCE} ORDER BY f.file_id,r.source_id,r.id"
    ))?;
    let mut input = select.query([])?;
    let mut blocks = (!differential).then(|| Blocks::new(c)).transpose()?;
    while let Some(row) = input.next()? {
        let (file, mut value) = read(row)?;
        let stage = value.id;
        let (id, retained) = prior.assign(&value)?;
        if retained {
            continue;
        }
        value.id = id;
        if let Some(assignments) = &mut assignments {
            assignments.execute(params![stage, id])?;
        } else if let Some(blocks) = &mut blocks {
            blocks.push(file, value.source, value)?;
        }
    }
    drop(input);
    drop(select);
    drop(assignments);
    drop(prior);
    if let Some(blocks) = &mut blocks {
        blocks.flush()?;
    }
    if differential {
        super::reconcile::remove_stale(c)?;
        let mut select = c.prepare(&format!(
            "SELECT {COLUMNS},a.id FROM {SOURCE}
            JOIN relation_assignments a ON a.stage=r.id ORDER BY f.file_id,r.source_id,a.id"
        ))?;
        let mut input = select.query([])?;
        let mut blocks = Blocks::new(c)?;
        while let Some(row) = input.next()? {
            let (file, value) = read(row)?;
            blocks.push(file, value.source, value)?;
        }
        blocks.flush()?;
        drop(input);
        drop(select);
        drop(blocks);
        c.execute_batch("DROP TABLE relation_assignments;")?;
    }
    Ok(())
}

fn read(row: &Row<'_>) -> Result<(i64, Relation)> {
    Ok((
        row.get(0)?,
        Relation {
            id: row.get(8)?,
            fact: row.get(1)?,
            source: row.get(2)?,
            target: row.get(3)?,
            external: row.get(4)?,
            state: Relation::state(row.get_ref(5)?.as_str()?)?,
            rule: row.get(6)?,
            reason: row.get(7)?,
        },
    ))
}
