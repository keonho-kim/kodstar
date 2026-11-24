// Purpose: 부분 구문 입력과 provider의 관계가 확정 결과로 공개되지 않게 제한.
// Pattern: Evidence Guard.
// Usage: resolver 준비와 관계 공개가 현재 세대의 복구 입력 집합을 조회.
// Related: crates/kodstar-core/src/application/analysis/resolve/publication.rs, crates/kodstar-core/src/infrastructure/index/failures.rs.

use super::{FileContext, Lookup};
use crate::{domain::diagnostics::Result, infrastructure::index::cached::Sql};

pub(super) fn prepare(c: &Sql<'_>) -> Result<bool> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS syntax_partial_files(id INTEGER PRIMARY KEY);
        DELETE FROM syntax_partial_files;
        INSERT OR IGNORE INTO syntax_partial_files SELECT file_id FROM diagnostics WHERE code IN ('source_syntax_partial','source_macro_incomplete');
        CREATE TEMP TABLE IF NOT EXISTS syntax_partial_symbols(id INTEGER PRIMARY KEY);
        DELETE FROM syntax_partial_symbols;
        WITH RECURSIVE tainted(id) AS (
            SELECT s.id FROM symbols s JOIN syntax_partial_files f ON f.id=s.file_id
            UNION SELECT a.canonical_id FROM symbol_aliases a JOIN tainted t ON t.id=a.symbol_id
            UNION SELECT a.symbol_id FROM symbol_aliases a JOIN tainted t ON t.id=a.canonical_id
            UNION SELECT s.id FROM symbols s JOIN tainted t ON t.id=s.owner_id)
        INSERT OR IGNORE INTO syntax_partial_symbols SELECT id FROM tainted;")?;
    Ok(c.cached_row(
        "SELECT EXISTS(SELECT 1 FROM syntax_partial_files)",
        [],
        |r| r.get(0),
    )?)
}

impl Lookup<'_, '_> {
    pub(super) fn recovered_source_candidates(&mut self, context: &FileContext) -> Result<()> {
        if !self.prepared.partial_sources.get() || self.rule == "clang_semantic_call" {
            return Ok(());
        }
        let uncertain: bool = self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM syntax_partial_files WHERE id=?1)
            OR EXISTS(SELECT 1 FROM resolution_candidates c JOIN syntax_partial_symbols s ON s.id=c.id)", [context.id], |r| r.get(0))?;
        if uncertain {
            self.candidate_only = true;
            self.reason = Some("source syntax recovery leaves binding or provider scope uncertain");
        }
        Ok(())
    }
}
