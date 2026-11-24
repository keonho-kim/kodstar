// Purpose: 보존된 정체성을 건드리지 않고 추가 심볼의 정체성·대표 객체만 계산.
// Pattern: Incremental Derived State.
// Usage: 부분 해석 준비가 append_symbols의 실제 소비 범위에 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/identities.rs, crates/kodstar-core/src/infrastructure/index/append_reuse/mod.rs.

use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result},
    infrastructure::index::cached::Sql as Connection,
};

pub(in crate::application::analysis::resolve) fn run(
    c: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    super::csharp::run(c, cancel, true)?;
    super::erlang::run(c, cancel, true)?;
    super::elixir::run(c, cancel, true)?;
    super::swift::run(c, cancel, true)?;
    rebuild(c)
}

pub(in crate::application::analysis::resolve) fn rebuild(c: &Connection) -> Result<()> {
    c.execute_batch("WITH duplicates(key) AS (
        SELECT identity FROM symbols_rows WHERE id IN (SELECT id FROM append_symbols)
        GROUP BY identity HAVING count(*)>1), representatives AS (
          SELECT s.id,first_value(s.id) OVER(PARTITION BY s.key_blob ORDER BY f.path,s.start_byte,s.id) AS canonical_id
          FROM duplicates d JOIN symbol_additions s ON s.key_blob=d.key JOIN files f ON f.id=s.file_id)
        INSERT OR REPLACE INTO symbol_aliases SELECT id,canonical_id FROM representatives WHERE id<>canonical_id;")?;
    Ok(())
}
