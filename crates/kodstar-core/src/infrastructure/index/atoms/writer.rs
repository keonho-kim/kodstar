// Purpose: 사실 writer 수명 안에서 작은 문자열 ID를 제한적으로 재사용.
// Pattern: Bounded Cache.
// Usage: FactWriter가 transaction의 추출 단계 동안만 소유하고 rollback 전에 해제.
// Related: crates/kodstar-core/src/infrastructure/index/facts.rs, crates/kodstar-core/src/infrastructure/index/atoms/schema.sql.

use rusqlite::{Connection, OptionalExtension, Result, Statement};
use std::collections::VecDeque;

pub(crate) struct Writer<'a> {
    find: Statement<'a>,
    insert: Statement<'a>,
    recent: VecDeque<(String, i64)>,
    bytes: usize,
    limit: usize,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(c: &'a Connection) -> Result<Self> {
        let limit =
            (c.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize / 32).min(65536);
        let recent =
            VecDeque::with_capacity((limit / (2 * std::mem::size_of::<(String, i64)>())).min(64));
        let bytes = recent.capacity() * std::mem::size_of::<(String, i64)>();
        Ok(Self {
            find: c.prepare("SELECT id FROM atoms WHERE value=?1")?,
            insert: c.prepare("INSERT INTO atoms(value) VALUES(?1) RETURNING id")?,
            recent,
            bytes,
            limit,
        })
    }
    pub(crate) fn intern(&mut self, value: &str) -> Result<i64> {
        if let Some((_, id)) = self.recent.iter().rev().find(|(v, _)| v == value) {
            return Ok(*id);
        }
        let found = self.find.query_row([value], |r| r.get(0)).optional();
        self.find.clear_bindings();
        let id = match found? {
            Some(id) => id,
            None => {
                let result = self.insert.query_row([value], |r| r.get(0));
                self.insert.clear_bindings();
                result?
            }
        };
        if value.len() <= 2048 && self.recent.capacity() > 0 {
            while !self.recent.is_empty()
                && (self.recent.len() >= 64
                    || self.recent.len() == self.recent.capacity()
                    || self.bytes.saturating_add(value.len()) > self.limit)
            {
                if let Some((old, _)) = self.recent.pop_front() {
                    self.bytes -= old.capacity();
                }
            }
            if self.bytes.saturating_add(value.len()) <= self.limit {
                let value = value.to_owned();
                self.bytes += value.capacity();
                self.recent.push_back((value, id));
            }
        }
        Ok(id)
    }
    pub(crate) fn optional(&mut self, value: Option<&str>) -> Result<Option<i64>> {
        value.map(|v| self.intern(v)).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_values_reuse_ids_and_retained_keys_stay_bounded() -> Result<()> {
        let c = Connection::open_in_memory()?;
        c.execute_batch(super::super::SCHEMA)?;
        let mut writer = Writer::new(&c)?;
        let id = writer.intern("shared")?;
        let writes = c.total_changes();
        assert_eq!(writer.intern("shared")?, id);
        assert_eq!(c.total_changes(), writes);
        for i in 0..2000 {
            writer.intern(&format!("{i}:{}", "x".repeat(1024)))?;
        }
        assert!(writer.recent.len() <= 64);
        assert!(writer.bytes <= writer.limit);
        Ok(())
    }
    #[test]
    fn a_large_value_is_not_retained_by_cache_or_bindings() -> Result<()> {
        let c = Connection::open_in_memory()?;
        c.execute_batch(super::super::SCHEMA)?;
        let mut writer = Writer::new(&c)?;
        let before = writer.insert.get_status(rusqlite::StatementStatus::MemUsed);
        writer.intern(&"x".repeat(300000))?;
        assert!(writer.recent.is_empty());
        assert!(writer.insert.get_status(rusqlite::StatementStatus::MemUsed) < before + 4096);
        Ok(())
    }
}
