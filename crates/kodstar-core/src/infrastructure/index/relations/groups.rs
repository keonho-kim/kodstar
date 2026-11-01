// Purpose: 관계 블록의 ID·fact 연결과 후보 그룹 경계를 제한된 메모리로 복원.
// Pattern: Bounded Reader.
// Usage: 행 decoder와 공유 집합 참조 검사가 같은 블록 형식을 읽음.
// Related: crates/kodstar-core/src/infrastructure/index/relations/codec.rs, crates/kodstar-core/src/infrastructure/index/relations/sets/mod.rs.

use super::sets::targets;
use crate::infrastructure::index::packed::codec::{self, Input};
use rusqlite::Result;
use std::{collections::BTreeSet, ops::Range};

pub(super) struct Groups<'a> {
    pub input: Input<'a>,
    pub rows: Vec<(i64, i64)>,
    at: usize,
}

impl<'a> Groups<'a> {
    pub fn new(bytes: &'a [u8], count: usize) -> Result<Self> {
        if !(1..=128).contains(&count) {
            return Err(codec::invalid());
        }
        let mut input = Input(bytes);
        let (mut id, mut fact) = (0i64, 0i64);
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let gap = input.id()?;
            id = id.checked_add(gap).ok_or_else(codec::invalid)?;
            let delta = input.integer()?;
            let delta = ((delta >> 1) as i64) ^ -((delta & 1) as i64);
            fact = fact.checked_add(delta).ok_or_else(codec::invalid)?;
            if gap == 0 || fact <= 0 {
                return Err(codec::invalid());
            }
            rows.push((id, fact));
        }
        Ok(Self { input, rows, at: 0 })
    }

    pub fn next(&mut self) -> Result<Option<(Range<usize>, i64)>> {
        if self.at == self.rows.len() {
            return Ok(None);
        }
        let count = usize::try_from(self.input.integer()?).map_err(|_| codec::invalid())?;
        let end = self
            .at
            .checked_add(count)
            .filter(|n| count > 0 && *n <= self.rows.len())
            .ok_or_else(codec::invalid)?;
        if self.rows[self.at..end]
            .iter()
            .any(|r| r.1 != self.rows[self.at].1)
        {
            return Err(codec::invalid());
        }
        let set = self.input.id()?;
        let range = self.at..end;
        self.at = end;
        Ok(Some((range, set)))
    }

    pub fn finish(self) -> Result<()> {
        if self.at != self.rows.len() {
            return Err(codec::invalid());
        }
        self.input.finish()
    }
}

pub(super) fn references(bytes: &[u8], count: usize) -> Result<Vec<i64>> {
    let mut groups = Groups::new(bytes, count)?;
    let mut ids = BTreeSet::new();
    while let Some((range, set)) = groups.next()? {
        if set > 0 {
            ids.insert(set);
        } else {
            for _ in range {
                targets::read(&mut groups.input)?;
            }
        }
    }
    groups.finish()?;
    Ok(ids.into_iter().collect())
}
