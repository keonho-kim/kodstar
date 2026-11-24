// Purpose: 제한된 Roaring 집합으로 방문·방향별 대기열 중복을 제거하고 디스크로 전환.
// Pattern: Bounded Working Set.
// Usage: traversal이 노드 방문을 승인하고 조회 종료에 함께 해제.
// Related: crates/kodstar-core/src/application/query/traversal.rs, crates/kodstar-core/src/infrastructure/index/cached.rs.

use crate::{
    domain::{
        cancellation::Cancellation,
        diagnostics::{Error, Result},
        results::QuerySummary,
    },
    infrastructure::index::cached::Sql,
};
use roaring::RoaringTreemap;
use std::collections::BTreeSet;

// A 16-bit group needs at most an 8 KiB bitmap. This conservative charge also
// covers array-to-bitmap conversion, vector growth, and both tree indexes.
const GROUP_BYTES: usize = 32 * 1024;

pub(super) struct Visited<'a, 'conn> {
    sql: &'a Sql<'conn>,
    nodes: Option<[RoaringTreemap; 3]>,
    groups: BTreeSet<u64>,
    bytes: usize,
    cancel: &'a Cancellation,
}

impl<'a, 'conn> Visited<'a, 'conn> {
    pub fn new(sql: &'a Sql<'conn>, bytes: usize, cancel: &'a Cancellation) -> Self {
        Self {
            sql,
            nodes: Some(std::array::from_fn(|_| RoaringTreemap::new())),
            groups: BTreeSet::new(),
            bytes,
            cancel,
        }
    }

    fn contains(&self, id: i64) -> Result<bool> {
        if id <= 0 {
            return Err(Error::operation("invalid graph node ID"));
        }
        if let Some(nodes) = &self.nodes {
            return Ok(nodes[0].contains(id as u64));
        }
        Ok(self.sql.cached_row(
            "SELECT EXISTS(SELECT 1 FROM q_nodes WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    fn remember(&mut self, id: i64, role: usize) -> Result<()> {
        let group = (role as u64) << 48 | id as u64 >> 16;
        if self.nodes.is_some() && !self.groups.contains(&group) {
            if self
                .groups
                .len()
                .saturating_add(1)
                .saturating_mul(GROUP_BYTES)
                > self.bytes
            {
                self.spill()?;
            } else {
                self.groups.insert(group);
            }
        }
        if let Some(nodes) = &mut self.nodes {
            nodes[role].insert(id as u64);
        } else if role == 0 {
            self.sql
                .cached_execute("INSERT INTO q_nodes VALUES(?1)", [id])?;
        }
        Ok(())
    }

    pub fn enqueue(&mut self, id: i64, direction: &str) -> Result<bool> {
        self.cancel.check()?;
        let role = match direction {
            "out" => 1,
            "in" => 2,
            _ => return Err(Error::operation("invalid traversal direction")),
        };
        if id <= 0 {
            return Err(Error::operation("invalid graph node ID"));
        }
        if self
            .nodes
            .as_ref()
            .is_some_and(|nodes| nodes[role].contains(id as u64))
        {
            return Ok(false);
        }
        // The frontier/seen tables already retain queue history. After spilling,
        // their existing INSERT OR IGNORE and NOT EXISTS checks own deduplication.
        self.remember(id, role)?;
        Ok(true)
    }

    fn spill(&mut self) -> Result<()> {
        if let Some(nodes) = self.nodes.take() {
            for (index, id) in nodes[0].iter().enumerate() {
                if index.is_multiple_of(256) {
                    self.cancel.check()?;
                }
                self.sql
                    .cached_execute("INSERT INTO q_nodes VALUES(?1)", [id as i64])?;
            }
        }
        self.groups.clear();
        Ok(())
    }

    pub fn admit(
        &mut self,
        id: Option<i64>,
        external: Option<&str>,
        maximum: Option<usize>,
        summary: &mut QuerySummary,
    ) -> Result<bool> {
        self.cancel.check()?;
        let exists = if let Some(id) = id {
            self.contains(id)?
        } else if let Some(name) = external {
            self.sql.cached_row(
                "SELECT EXISTS(SELECT 1 FROM q_externals WHERE name=?1)",
                [name],
                |r| r.get(0),
            )?
        } else {
            return Ok(true);
        };
        if exists {
            return Ok(true);
        }
        if maximum.is_some_and(|limit| summary.visited_count >= limit as u64) {
            summary.complete = false;
            summary.truncated = true;
            if !summary.reasons.contains(&"visit_limit") {
                summary.reasons.push("visit_limit");
            }
            return Ok(false);
        }
        if let Some(id) = id {
            self.remember(id, 0)?;
        } else if let Some(name) = external {
            self.sql
                .cached_execute("INSERT INTO q_externals VALUES(?1)", [name])?;
        }
        summary.visited_count += 1;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spilling_preserves_large_ids_duplicates_externals_and_visit_limits() -> Result<()> {
        for budget in [0, GROUP_BYTES, 2 * GROUP_BYTES] {
            let connection = rusqlite::Connection::open_in_memory()?;
            connection.execute_batch("CREATE TEMP TABLE q_nodes(id INTEGER PRIMARY KEY);CREATE TEMP TABLE q_externals(name TEXT PRIMARY KEY);")?;
            let sql = Sql::new(&connection)?;
            let cancel = Cancellation::default();
            let mut visited = Visited::new(&sql, budget, &cancel);
            let mut summary = QuerySummary {
                complete: true,
                ..QuerySummary::default()
            };
            for id in [1, 2, i64::MAX, 1, i64::MAX] {
                assert!(visited.admit(Some(id), None, Some(4), &mut summary)?);
            }
            assert!(visited.admit(None, Some("external"), Some(4), &mut summary)?);
            assert!(visited.admit(None, Some("external"), Some(4), &mut summary)?);
            assert!(!visited.admit(Some(3), None, Some(4), &mut summary)?);
            assert!(visited.contains(1)?);
            assert!(visited.contains(i64::MAX)?);
            assert_eq!(summary.visited_count, 4);
            assert_eq!(summary.reasons, ["visit_limit"]);
            assert!(summary.truncated && !summary.complete);
        }
        Ok(())
    }

    #[test]
    fn cancellation_during_spill_stops_before_the_working_set_is_copied() -> Result<()> {
        let connection = rusqlite::Connection::open_in_memory()?;
        connection.execute_batch("CREATE TEMP TABLE q_nodes(id INTEGER PRIMARY KEY);CREATE TEMP TABLE q_externals(name TEXT PRIMARY KEY);")?;
        let cancel = Cancellation::default();
        let token = cancel.clone();
        connection.update_hook(Some(
            move |_: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
                if table == "q_nodes" {
                    token.cancel();
                }
            },
        ))?;
        let sql = Sql::new(&connection)?;
        let mut visited = Visited::new(&sql, GROUP_BYTES, &cancel);
        let mut summary = QuerySummary::default();
        for id in 1..=1024 {
            assert!(visited.admit(Some(id), None, None, &mut summary)?);
        }
        let error = visited
            .admit(Some(1 << 40), None, None, &mut summary)
            .unwrap_err();
        assert_eq!(error.kind, crate::domain::diagnostics::ErrorKind::Cancelled);
        let copied: usize =
            connection.query_row("SELECT count(*) FROM q_nodes", [], |row| row.get(0))?;
        assert!(copied > 0 && copied <= 256);
        assert_eq!(summary.visited_count, 1024);
        Ok(())
    }

    #[test]
    fn queue_deduplication_keeps_directions_separate_and_defers_to_sql_after_spilling() -> Result<()>
    {
        let connection = rusqlite::Connection::open_in_memory()?;
        connection.execute_batch("CREATE TEMP TABLE q_nodes(id INTEGER PRIMARY KEY);CREATE TEMP TABLE q_externals(name TEXT PRIMARY KEY);")?;
        let sql = Sql::new(&connection)?;
        let cancel = Cancellation::default();
        let mut visited = Visited::new(&sql, GROUP_BYTES * 3, &cancel);
        let mut summary = QuerySummary::default();
        visited.admit(Some(1), None, None, &mut summary)?;
        assert!(visited.enqueue(1, "out")?);
        assert!(!visited.enqueue(1, "out")?);
        assert!(visited.enqueue(1, "in")?);
        assert!(visited.enqueue(i64::MAX, "out")?);
        assert!(visited.nodes.is_none());
        assert!(visited.contains(1)?);
        assert!(visited.enqueue(1, "out")?);
        Ok(())
    }
}
