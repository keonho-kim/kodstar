// Purpose: 고정 필드 관계를 압축 블록으로 저장·선택 복원.
// Pattern: Bounded Encoding and Visitor.
// Usage: 관계 writer·cursor·무결성 검사가 공유.
// Related: crates/kodstar-core/src/infrastructure/index/relations/row.rs, crates/kodstar-core/src/infrastructure/index/relations/decode.rs.

use super::super::packed::{Filter, codec};
use super::{Relation, decode, sets};
use rusqlite::{Connection, Result};

pub(super) fn encode(rows: &[Relation], sets: &mut sets::Writer<'_>) -> Result<Vec<u8>> {
    if !(1..=128).contains(&rows.len()) {
        return Err(codec::invalid());
    }
    let (mut bytes, mut previous, mut fact) = (Vec::new(), 0i64, 0i64);
    for row in rows {
        row.validate()?;
        if row.id <= previous {
            return Err(codec::invalid());
        }
        codec::integer(&mut bytes, (row.id - previous) as u64);
        let delta = row.fact - fact;
        codec::integer(&mut bytes, ((delta << 1) ^ (delta >> 63)) as u64);
        previous = row.id;
        fact = row.fact;
    }
    let mut start = 0;
    while start < rows.len() {
        let count = rows[start..]
            .iter()
            .take_while(|r| r.fact == rows[start].fact)
            .count();
        let targets = sets::targets::encode(&rows[start..start + count])?;
        let set = sets.intern(&targets, count)?;
        codec::integer(&mut bytes, count as u64);
        codec::integer(&mut bytes, set as u64);
        if set == 0 {
            bytes.extend_from_slice(&targets);
        }
        start += count;
    }
    Ok(bytes)
}

pub(super) fn decode(
    c: &Connection,
    bytes: &[u8],
    count: usize,
    source: i64,
) -> Result<Vec<Relation>> {
    select(c, bytes, count, source, &[])
}

pub(super) fn select(
    c: &Connection,
    bytes: &[u8],
    count: usize,
    source: i64,
    filters: &[Filter],
) -> Result<Vec<Relation>> {
    let mut rows = Vec::with_capacity(if filters.is_empty() {
        count.min(128)
    } else {
        0
    });
    decode::visit(c, bytes, count, source, |row| {
        if filters
            .iter()
            .all(|(column, value)| super::super::packed::matches(row.value(*column), value))
        {
            rows.push(row.into_owned());
        }
        Ok(())
    })?;
    Ok(rows)
}
