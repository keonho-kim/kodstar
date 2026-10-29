// Purpose: 재해석 전후 동일한 관계의 ID를 재사용해 검색 경로와 블록의 변경량 제한.
// Pattern: Differential Publication.
// Usage: 부분 무효화가 이전 행을 디스크에 보존하고 publication이 정확히 대조.
// Related: crates/kodstar-core/src/infrastructure/index/relations/removal.rs, crates/kodstar-core/src/infrastructure/index/relations/publication.rs.

use super::Relation;
use rusqlite::{Connection, OptionalExtension, Result, Statement, params};

pub(in crate::infrastructure::index) const SCHEMA: &str =
    "CREATE TEMP TABLE IF NOT EXISTS relation_prior(
    id INTEGER PRIMARY KEY,fact_id INTEGER,source_id INTEGER,target_id INTEGER,external_key TEXT,
    resolution TEXT,rule_id INTEGER,reason_id INTEGER);
    CREATE INDEX IF NOT EXISTS temp.relation_prior_fact ON relation_prior(fact_id,source_id);";

pub(super) const INSERT: &str = "INSERT INTO relation_prior VALUES(?1,?2,?3,?4,?5,?6,?7,?8)";
pub(super) const MAX_ID: &str = "SELECT max(coalesce((SELECT last FROM relation_id_ranges ORDER BY first DESC LIMIT 1),0),coalesce((SELECT max(id) FROM relation_prior),0))";

pub(super) struct Prior<'a> {
    consume: Statement<'a>,
    active: bool,
    next: i64,
}

impl<'a> Prior<'a> {
    pub fn new(c: &'a Connection) -> Result<Self> {
        Ok(Self {
            consume: c.prepare(
                "DELETE FROM relation_prior WHERE id=(SELECT id FROM relation_prior WHERE fact_id=?1 AND source_id=?2
                AND target_id IS ?3 AND external_key IS ?4 AND resolution=?5 AND rule_id=?6
                AND reason_id IS ?7 ORDER BY id LIMIT 1) RETURNING id",
            )?,
            active: c.query_row("SELECT EXISTS(SELECT 1 FROM relation_prior)", [], |r| {
                r.get(0)
            })?,
            next: c.query_row(MAX_ID, [], |r| r.get(0))?,
        })
    }

    pub fn assign(&mut self, row: &Relation) -> Result<i64> {
        if self.active {
            let prior = self
                .consume
                .query_row(
                    params![
                        row.fact,
                        row.source,
                        row.target,
                        row.external,
                        row.state.as_str(),
                        row.rule,
                        row.reason
                    ],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            if let Some(id) = prior {
                return Ok(id);
            }
        }
        self.next = self.next.checked_add(1).ok_or_else(super::invalid)?;
        Ok(self.next)
    }
}

pub(super) fn remember(statement: &mut Statement<'_>, row: &Relation) -> Result<()> {
    statement.execute(params![
        row.id,
        row.fact,
        row.source,
        row.target,
        row.external,
        row.state.as_str(),
        row.rule,
        row.reason
    ])?;
    Ok(())
}
