// Purpose: 전체 블록을 검증하면서 빌린 관계 행을 소비자에게 순차 전달.
// Pattern: Visitor.
// Usage: codec의 전체 읽기와 조건 선택이 같은 decoder를 사용.
// Related: crates/kodstar-core/src/infrastructure/index/relations/codec.rs, crates/kodstar-core/src/infrastructure/index/relations/row.rs.

use super::super::packed::codec::{self, Input};
use super::{Relation, groups::Groups, sets::targets};
use crate::domain::relations::Resolution;
use rusqlite::{Connection, Result, Statement, types::ValueRef};

pub(super) struct Borrowed<'a> {
    id: i64,
    fact: i64,
    source: i64,
    target: Option<i64>,
    external: Option<&'a str>,
    state: Resolution,
    rule: i64,
    reason: Option<i64>,
}

impl Borrowed<'_> {
    pub fn value(&self, column: usize) -> ValueRef<'_> {
        match column {
            0 => ValueRef::Integer(self.id),
            1 => ValueRef::Integer(self.fact),
            2 => ValueRef::Integer(self.source),
            3 => self.target.map_or(ValueRef::Null, ValueRef::Integer),
            4 => self
                .external
                .map_or(ValueRef::Null, |s| ValueRef::Text(s.as_bytes())),
            5 => ValueRef::Text(self.state.as_str().as_bytes()),
            6 => ValueRef::Integer(self.rule),
            7 => self.reason.map_or(ValueRef::Null, ValueRef::Integer),
            _ => ValueRef::Null,
        }
    }

    pub fn into_owned(self) -> Relation {
        Relation {
            id: self.id,
            fact: self.fact,
            source: self.source,
            target: self.target,
            external: self.external.map(str::to_owned),
            state: self.state,
            rule: self.rule,
            reason: self.reason,
        }
    }
}

pub(super) fn visit(
    c: &Connection,
    bytes: &[u8],
    count: usize,
    source: i64,
    mut consume: impl FnMut(Borrowed<'_>) -> Result<()>,
) -> Result<()> {
    if source <= 0 {
        return Err(codec::invalid());
    }
    let mut groups = Groups::new(bytes, count)?;
    let mut read: Option<Statement<'_>> = None;
    while let Some((range, set)) = groups.next()? {
        if set == 0 {
            rows(&mut groups.input, &groups.rows[range], source, &mut consume)?;
        } else {
            let query = match &mut read {
                Some(query) => query,
                slot @ None => {
                    slot.insert(c.prepare("SELECT count,payload FROM relation_sets WHERE id=?1")?)
                }
            };
            let payload: Vec<u8> = query.query_row([set], |r| {
                if r.get::<_, usize>(0)? != range.len() {
                    return Err(codec::invalid());
                }
                r.get(1)
            })?;
            let mut input = Input(&payload);
            rows(&mut input, &groups.rows[range], source, &mut consume)?;
            input.finish()?;
        }
    }
    groups.finish()
}

fn rows(
    input: &mut Input<'_>,
    associations: &[(i64, i64)],
    source: i64,
    consume: &mut impl FnMut(Borrowed<'_>) -> Result<()>,
) -> Result<()> {
    for &(id, fact) in associations {
        let target = targets::read(input)?;
        consume(Borrowed {
            id,
            fact,
            source,
            target: target.target,
            external: target.external,
            state: target.state,
            rule: target.rule,
            reason: target.reason,
        })?;
    }
    Ok(())
}
