// Purpose: 명시 전환·검증의 조상 판정을 디스크 skip 인덱스로 제한.
// Pattern: Temporary Index.
// Usage: metadata 변환·검증이 작업 동안 소유하고 마지막에 표를 제거.
// Related: crates/kodstar-core/src/infrastructure/history/maintenance/metadata/mod.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::{Connection, Statement, params};

pub(super) struct Ancestry<'a> {
    read: Statement<'a>,
}
impl<'a> Ancestry<'a> {
    pub fn new(c: &'a Connection, cancel: &Cancellation) -> Result<Self> {
        c.execute_batch("CREATE TEMP TABLE metadata_ancestry(id INTEGER PRIMARY KEY,parent INTEGER,depth INTEGER,skip INTEGER);")?;
        let mut ancestry = Self {
            read: c.prepare("SELECT parent,depth,skip FROM metadata_ancestry WHERE id=?1")?,
        };
        let mut select = c.prepare("SELECT id,parent FROM checkpoints ORDER BY id")?;
        let mut rows = select.query([])?;
        let mut insert = c.prepare("INSERT INTO metadata_ancestry VALUES(?1,?2,?3,?4)")?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let id: i64 = row.get(0)?;
            let parent: Option<i64> = row.get(1)?;
            let (depth, skip) = if let Some(parent) = parent {
                if parent >= id {
                    return Err(invalid());
                }
                let depth = ancestry
                    .node(parent)?
                    .1
                    .checked_add(1)
                    .ok_or_else(invalid)?;
                (depth, Some(ancestry.at(parent, depth & (depth - 1))?))
            } else {
                (0, None)
            };
            insert.execute(params![id, parent, depth, skip])?;
        }
        Ok(ancestry)
    }
    fn node(&mut self, id: i64) -> Result<(Option<i64>, i64, Option<i64>)> {
        Ok(self
            .read
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?)
    }
    fn at(&mut self, mut id: i64, wanted: i64) -> Result<i64> {
        for _ in 0..128 {
            let (parent, depth, skip) = self.node(id)?;
            if depth == wanted {
                return Ok(id);
            }
            if depth < wanted {
                return Err(invalid());
            }
            let next = if depth & (depth - 1) >= wanted {
                skip.or(parent)
            } else {
                parent
            }
            .ok_or_else(invalid)?;
            if next >= id {
                return Err(invalid());
            }
            id = next;
        }
        Err(invalid())
    }
    pub fn contains(&mut self, descendant: i64, ancestor: i64) -> Result<bool> {
        if descendant == ancestor {
            return Ok(true);
        }
        if ancestor > descendant {
            return Ok(false);
        }
        let target = self.node(ancestor)?.1;
        if self.node(descendant)?.1 < target {
            return Ok(false);
        }
        Ok(self.at(descendant, target)? == ancestor)
    }
}

fn invalid() -> Error {
    Error::operation("invalid history metadata ancestry")
}
