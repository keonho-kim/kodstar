// Purpose: 고정 필드 관계를 압축 블록으로 저장·선택 복원.
// Pattern: Bounded Encoding and Visitor.
// Usage: 관계 writer·cursor·무결성 검사가 공유.
// Related: crates/kodstar-core/src/infrastructure/index/relations/row.rs, crates/kodstar-core/src/infrastructure/index/relations/decode.rs.

use super::super::packed::{Filter, codec};
use super::{Relation, decode};
use crate::domain::relations::Resolution;
use rusqlite::Result;

pub(super) fn encode(rows: &[Relation]) -> Result<Vec<u8>> {
    let (mut bytes, mut previous, mut fact) = (Vec::new(), 0i64, 0i64);
    for row in rows {
        row.validate()?;
        if row.id <= previous {
            return Err(codec::invalid());
        }
        codec::integer(&mut bytes, (row.id - previous) as u64);
        let delta = row.fact - fact;
        codec::integer(&mut bytes, ((delta << 1) ^ (delta >> 63)) as u64);
        for id in [row.target, Some(row.rule), row.reason] {
            codec::integer(&mut bytes, id.unwrap_or(0) as u64);
        }
        let state = match row.state {
            Resolution::Resolved => 0,
            Resolution::Candidate => 1,
            Resolution::Ambiguous => 2,
            Resolution::Unresolved => 3,
        };
        codec::integer(&mut bytes, state * 2 + u64::from(row.external.is_some()));
        if let Some(text) = &row.external {
            codec::text(&mut bytes, text);
        }
        previous = row.id;
        fact = row.fact;
    }
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8], count: usize, source: i64) -> Result<Vec<Relation>> {
    select(bytes, count, source, &[])
}

pub(super) fn select(
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
    decode::visit(bytes, count, source, |row| {
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
