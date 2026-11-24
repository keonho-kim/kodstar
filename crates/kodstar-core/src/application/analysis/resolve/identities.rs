// Purpose: 언어별 논리 정체성·대표 객체와 새 공개 후보의 대응 계산.
// Pattern: Simple Module.
// Usage: 매 갱신에서 lexical 해석 전에 실행하며 원래 구문 행은 보존.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/publication.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;
mod added;
mod csharp;
mod elixir;
mod erlang;
mod swift;
pub(super) use added::rebuild as rebuild_added;
pub(super) use added::run as added;

pub(super) fn run(c: &Connection, cancel: &Cancellation) -> Result<()> {
    c.execute_batch("DELETE FROM symbol_aliases; UPDATE symbols SET resolved_key=NULL,resolved_qualified_name=NULL,resolved_kind=NULL WHERE resolved_key IS NOT NULL OR resolved_qualified_name IS NOT NULL OR resolved_kind IS NOT NULL;")?;
    csharp::run(c, cancel, false)?;
    erlang::run(c, cancel, false)?;
    elixir::run(c, cancel, false)?;
    swift::run(c, cancel, false)?;
    rebuild(c)?;
    c.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS resolution_canonical(id INTEGER PRIMARY KEY);",
    )?;
    Ok(())
}

pub(super) fn rebuild(c: &Connection) -> Result<()> {
    c.execute_batch(
        "DELETE FROM symbol_aliases;
        WITH duplicates(key) AS (SELECT identity FROM symbols_rows GROUP BY identity HAVING count(*)>1),
        representatives AS (
          SELECT s.id,first_value(s.id) OVER(PARTITION BY s.key_blob ORDER BY f.path,s.start_byte,s.id) AS canonical_id
          FROM duplicates d JOIN symbols s ON s.key_blob=d.key JOIN files f ON f.id=s.file_id)
        INSERT INTO symbol_aliases SELECT id,canonical_id FROM representatives WHERE id<>canonical_id;",
    )?;
    Ok(())
}

pub(super) fn candidates(c: &Connection) -> Result<()> {
    c.execute_batch(
        "DELETE
        FROM resolution_canonical; INSERT
        OR IGNORE INTO resolution_canonical
        SELECT g.canonical_id
        FROM resolution_candidates r
        JOIN symbol_identities g ON g.symbol_id=r.id; DELETE
        FROM resolution_candidates; INSERT INTO resolution_candidates
        SELECT id
        FROM resolution_canonical;",
    )?;
    Ok(())
}

pub(super) fn publication(c: &Connection) -> Result<()> {
    let merged: bool = c.cached_row("SELECT EXISTS(SELECT 1 FROM symbol_aliases)", [], |r| {
        r.get(0)
    })?;
    if merged {
        c.execute_batch("UPDATE resolution_relationships SET
            source_id=COALESCE((SELECT canonical_id FROM symbol_aliases WHERE symbol_id=resolution_relationships.source_id),source_id),
            target_id=COALESCE((SELECT canonical_id FROM symbol_aliases WHERE symbol_id=resolution_relationships.target_id),target_id)
            WHERE source_id IN (SELECT symbol_id FROM symbol_aliases) OR target_id IN (SELECT symbol_id FROM symbol_aliases);")?;
    }
    Ok(())
}
