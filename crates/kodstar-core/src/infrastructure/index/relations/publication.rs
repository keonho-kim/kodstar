// Purpose: 해석 staging의 관계를 파일·source별 제한 블록으로 일괄 공개.
// Pattern: Streaming Publication.
// Usage: resolver flush가 같은 분석 transaction 안에서 호출.
// Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs, crates/kodstar-core/src/application/analysis/resolve/publication.rs.

use super::super::packed::Row;
use rusqlite::{Connection, Result, types::Value};

pub(crate) fn publish(c: &Connection) -> Result<()> {
    c.execute_batch("INSERT OR IGNORE INTO relation_texts(value) SELECT rule FROM resolution_relationships UNION SELECT reason FROM resolution_relationships WHERE reason IS NOT NULL;")?;
    let mut select=c.prepare("SELECT f.file_id,r.fact_id,r.source_id,r.target_id,r.external_key,r.resolution,t.id,reason.id FROM resolution_relationships r JOIN facts f ON f.id=r.fact_id JOIN relation_texts t ON t.value=r.rule LEFT JOIN relation_texts reason ON reason.value=r.reason ORDER BY f.file_id,r.source_id,r.id")?;
    let mut input = select.query([])?;
    let mut next: i64 = c.query_row("SELECT coalesce(max(id),0) FROM relation_ids", [], |r| {
        r.get(0)
    })?;
    let mut current = None;
    let mut rows: Vec<Row> = Vec::with_capacity(128);
    let mut bytes = 0;
    while let Some(row) = input.next()? {
        let group = (row.get::<_, i64>(0)?, row.get::<_, i64>(2)?);
        let mut values = Vec::with_capacity(8);
        for column in 0..=7 {
            values.push(row.get::<_, Value>(column)?);
        }
        let row_bytes = values
            .iter()
            .map(|v| if let Value::Text(s) = v { s.len() } else { 8 })
            .sum::<usize>();
        if current != Some(group)
            || rows.len() == 128
            || !rows.is_empty() && bytes + row_bytes > 65536
        {
            if let Some((file, source)) = current {
                super::store(c, 0, file, source, &rows)?;
            }
            rows.clear();
            bytes = 0;
            current = Some(group);
        }
        next = next.checked_add(1).ok_or_else(super::invalid)?;
        values[0] = Value::Integer(next);
        bytes += row_bytes;
        rows.push(values);
    }
    if let Some((file, source)) = current {
        super::store(c, 0, file, source, &rows)?;
    }
    Ok(())
}
