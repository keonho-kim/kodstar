// Purpose: 해석 증거·관계를 디스크 작업 집합에 기록하고 같은 세대에 공개.
// Pattern: Simple Module.
// Usage: resolve가 fact별 결과를 쓰고 읽기 cursor 종료 후 flush.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/infrastructure/index/schema.sql.

use crate::application::analysis::resolve::prepared::Publication;
use crate::application::analysis::resolve::{FileContext, Lookup, identities};
use crate::domain::diagnostics::{Error, Result};
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

pub(super) fn reset(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolution_dependencies(fact_id INTEGER,kind TEXT,lookup_key TEXT,found INTEGER,PRIMARY KEY(fact_id,kind,lookup_key)) WITHOUT ROWID;
      CREATE TEMP TABLE IF NOT EXISTS resolution_relationships(id INTEGER PRIMARY KEY,fact_id INTEGER,source_id INTEGER,target_id INTEGER,external_key TEXT,resolution TEXT,rule TEXT,reason TEXT);
      DELETE FROM resolution_dependencies; DELETE FROM resolution_relationships;")?;
    Ok(())
}

pub(super) fn flush(c: &Connection) -> Result<()> {
    crate::infrastructure::index::dependencies::publish(c)?;
    crate::infrastructure::index::relations::publish(c)?;
    crate::infrastructure::index::dictionary::retire(c)?;
    c.execute_batch("DELETE FROM resolution_dependencies; DELETE FROM resolution_relationships;")?;
    Ok(())
}

impl Lookup<'_, '_> {
    pub(super) fn publish(
        &mut self,
        context: &FileContext,
        scope: u32,
        source: i64,
        expression: &str,
        merged: bool,
    ) -> Result<()> {
        if self.external_scope_id != 0 {
            return Err(Error::operation(
                "cannot publish an unfinished external lookup scope",
            ));
        }
        let fact = self
            .fact
            .ok_or_else(|| Error::operation("cannot publish lookup without a syntax fact"))?;
        let mut source = self.source_override.unwrap_or(source);
        if merged {
            identities::candidates(self.connection)?;
            source = self.connection.cached_row(
                "SELECT canonical_id FROM symbol_identities WHERE symbol_id=?1",
                [source],
                |r| r.get(0),
            )?;
        }
        let count: i64 =
            self.connection
                .cached_row("SELECT COUNT(*) FROM resolution_candidates", [], |r| {
                    r.get(0)
                })?;
        self.prepared.dependency(params![
            fact,
            "expression",
            format!(
                "{}:{}:{scope}:{expression}",
                context.language.as_str(),
                context.module
            ),
            count > 0
        ])?;
        let external_count: i64 = self.connection.cached_row(
            "SELECT COUNT(*) FROM resolution_externals WHERE scope=0",
            [],
            |r| r.get(0),
        )?;
        let ambiguous = count + external_count > 1;
        // Only base relations are consumed during this phase. Other consumers run after flush.
        let base = matches!(self.relation, "inherits" | "implements" | "mixes_in");
        if count > 0 {
            let state = if ambiguous {
                "ambiguous"
            } else if self.candidate_only {
                "candidate"
            } else {
                "resolved"
            };
            let reason = if ambiguous {
                Some("multiple internal or external matching definitions")
            } else if self.candidate_only {
                self.reason
            } else {
                None
            };
            self.prepared.publish(
                base,
                Publication::Internal,
                params![fact, source, state, self.rule, reason],
            )?;
        }
        if external_count > 0 {
            let state = if ambiguous {
                "ambiguous"
            } else if self.candidate_only {
                "candidate"
            } else if matches!(self.relation, "imports" | "reexports")
                || matches!(self.rule, "language_builtin" | "manifest_dependency")
            {
                "resolved"
            } else {
                "candidate"
            };
            let reason = if ambiguous {
                "multiple internal or external matching definitions"
            } else {
                self.reason.unwrap_or("external target is not indexed")
            };
            self.prepared.publish(
                base,
                Publication::External,
                params![fact, source, state, self.rule, reason],
            )?;
        } else if count == 0 {
            self.prepared.publish(
                base,
                Publication::Unresolved,
                params![
                    fact,
                    source,
                    self.rule,
                    self.reason
                        .unwrap_or("no matching binding or explicit module definition")
                ],
            )?;
        }
        Ok(())
    }
}
