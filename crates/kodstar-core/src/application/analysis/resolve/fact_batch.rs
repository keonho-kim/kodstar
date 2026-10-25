// Purpose: 제한된 사실 배치를 읽고 해석·쓰기 전에 SQLite cursor를 닫음.
// Pattern: Bounded Batch.
// Usage: facts resolver가 ID 순서와 두 해석 단계를 보존해 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/facts.rs, crates/kodstar-core/src/infrastructure/index/runtime.rs.

use rusqlite::{Result, Statement};

pub(super) struct Input {
    pub fact: i64,
    pub file: i64,
    pub scope: u32,
    pub expression: String,
    pub relation: String,
    pub position: usize,
    pub source: i64,
    pub language: String,
}

pub(super) fn read(statement: &mut Statement<'_>, after: i64, budget: usize) -> Result<Vec<Input>> {
    let capacity = (budget / std::mem::size_of::<Input>()).clamp(1, 128);
    let mut batch = Vec::with_capacity(capacity);
    let mut bytes = batch.capacity() * std::mem::size_of::<Input>();
    let mut rows = statement.query(rusqlite::params![after, capacity])?;
    while let Some(row) = rows.next()? {
        let payload = [3, 4, 7]
            .into_iter()
            .map(|i| -> Result<usize> { Ok(row.get_ref(i)?.as_str()?.len()) })
            .sum::<Result<usize>>()?;
        // Preserve one accepted large fact; never retain a second row beyond the batch budget.
        if !batch.is_empty() && bytes.saturating_add(payload) > budget {
            break;
        }
        let item = Input {
            fact: row.get(0)?,
            file: row.get(1)?,
            scope: row.get(2)?,
            expression: row.get(3)?,
            relation: row.get(4)?,
            position: row.get(5)?,
            source: row.get(6)?,
            language: row.get(7)?,
        };
        bytes = bytes.saturating_add(
            item.expression.capacity() + item.relation.capacity() + item.language.capacity(),
        );
        batch.push(item);
    }
    Ok(batch)
}
