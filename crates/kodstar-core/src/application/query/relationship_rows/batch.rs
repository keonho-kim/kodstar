// Purpose: Prefetch the bounded facts and endpoints of already selected relationship rows.
// Pattern: Batched Storage Reader.
// Usage: query/relationships primes Reader before emitting a batch in its original order.
// Related: crates/kodstar-core/src/application/query/relationship_rows.rs, crates/kodstar-core/src/infrastructure/index/reader.rs.

use super::{Reader, StoredRelationship};
use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result},
    infrastructure::index::BATCH_IDS,
};
use rusqlite::params_from_iter;
use std::sync::Arc;

impl Reader<'_> {
    pub(in crate::application::query) fn prefetch(
        &mut self,
        ids: &[i64],
        cancel: &Cancellation,
    ) -> Result<()> {
        self.facts.clear();
        self.selected.clear();
        self.pending = None;
        if ids.len() < 4 {
            return Ok(());
        }
        if self.batch_statement.is_none() {
            self.batch_statement = Some(self.connection.prepare(&format!(
                "{BATCH_IDS} SELECT f.relation,
                fi.display_path,fi.path,f.start_byte,f.end_byte,f.line,f.column_byte,f.end_line,
                f.end_column_byte,f.condition,f.type_only,f.expression,f.id
                FROM facts f JOIN files fi ON fi.id=f.file_id
                WHERE f.id IN (SELECT id FROM batch) ORDER BY f.id"
            ))?);
        }
        let mut facts = Vec::with_capacity(32);
        let mut symbols = Vec::with_capacity(64);
        let mut retained = 32 * std::mem::size_of::<super::Relation>();
        for &id in ids {
            cancel.check()?;
            let row = self.relations.read(id)?;
            retained = retained.saturating_add(row.external.as_ref().map_or(0, String::capacity));
            if retained > self.fact_limit {
                // Keep one accepted oversized row for its ordered read and stop
                // prefetching; the cache budget must not reduce the result scope.
                self.pending = Some(row);
                break;
            }
            facts.push(row.fact);
            symbols.push(row.source);
            if let Some(target) = row.target {
                symbols.push(target);
            }
            self.selected.push_back(row);
        }
        symbols.sort_unstable();
        symbols.dedup();
        self.symbols.prefetch(&symbols, cancel)?;
        let parameters = std::array::from_fn::<_, 64, _>(|at| facts.get(at).copied());
        let mut rows = self
            .batch_statement
            .as_mut()
            .unwrap()
            .query(params_from_iter(parameters))?;
        let mut used = 32 * std::mem::size_of::<(i64, Arc<StoredRelationship>)>();
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let (stored, raw) = StoredRelationship::from_row(row)?;
            let stored = stored.with_path(raw)?;
            used = used.saturating_add(stored.owned_bytes() + 2 * std::mem::size_of::<usize>());
            if used > self.fact_limit {
                break;
            }
            self.facts.push((row.get(12)?, stored));
        }
        Ok(())
    }
}
