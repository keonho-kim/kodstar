// Purpose: 수집에서 확인한 기존 경로를 제한된 바이트로 보유하고 삭제를 판정.
// Pattern: Bounded Working Set.
// Usage: capture가 이전 catalog의 경로만 전달하고 누락이 있을 때 대조.
// Related: crates/kodstar-core/src/application/history/capture.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
};
use rusqlite::Connection;

pub(super) struct Seen {
    paths: Vec<Vec<u8>>,
    count: u64,
    bytes: usize,
    budget: usize,
    spilled: bool,
}

impl Seen {
    pub(super) fn new(budget: usize) -> Self {
        Self {
            paths: Vec::new(),
            count: 0,
            bytes: 0,
            budget,
            spilled: false,
        }
    }

    pub(super) fn push(
        &mut self,
        c: &Connection,
        path: Vec<u8>,
        cancel: &Cancellation,
    ) -> Result<()> {
        self.count += 1;
        // Vec grows geometrically. Budget two slots per retained path in addition to its bytes.
        self.bytes = self
            .bytes
            .saturating_add(path.capacity() + 2 * std::mem::size_of::<Vec<u8>>());
        if !self.spilled && self.bytes > self.budget {
            self.spill(c, cancel)?;
        }
        if self.spilled {
            c.prepare_cached("INSERT INTO seen_paths VALUES(?1)")?
                .execute([path])?;
        } else {
            self.paths.push(path);
        }
        Ok(())
    }

    fn spill(&mut self, c: &Connection, cancel: &Cancellation) -> Result<()> {
        c.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS seen_paths(path BLOB PRIMARY KEY) WITHOUT ROWID;
                         DELETE FROM seen_paths;",
        )?;
        let mut insert = c.prepare_cached("INSERT INTO seen_paths VALUES(?1)")?;
        for path in std::mem::take(&mut self.paths) {
            cancel.check()?;
            insert.execute([path])?;
        }
        self.spilled = true;
        Ok(())
    }

    pub(super) fn deleted(
        mut self,
        c: &Connection,
        previous: u64,
        cancel: &Cancellation,
    ) -> Result<u64> {
        let missing = previous
            .checked_sub(self.count)
            .ok_or_else(|| Error::operation("repeated history path observation"))?;
        if missing == 0 {
            if self.spilled {
                c.execute_batch("DROP TABLE seen_paths;")?;
            }
            return Ok(0);
        }
        if !self.spilled {
            self.spill(c, cancel)?;
        }
        c.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS deleted_paths(path BLOB PRIMARY KEY) WITHOUT ROWID;
                         DELETE FROM deleted_paths;",
        )?;
        let found = c.execute(
            "INSERT INTO deleted_paths SELECT path FROM working
                               WHERE path NOT IN (SELECT path FROM seen_paths)",
            [],
        )?;
        c.execute_batch("DROP TABLE seen_paths;")?;
        if found as u64 != missing {
            return Err(Error::operation("history scan and deletion counts differ"));
        }
        Ok(missing)
    }
}
