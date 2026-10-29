// Purpose: 언어 공통 본문 재해석이 전역 파생 상태를 바꾸지 않는 조건 판정.
// Pattern: Specification.
// Usage: ReusePolicy가 환경 fingerprint 일치 후 한 세대에 한 번 호출.
// Related: crates/kodstar-core/src/application/analysis/incremental.rs, crates/kodstar-core/src/domain/facts/fingerprint.rs.

use crate::domain::diagnostics::Result;
use rusqlite::Connection;

pub(super) fn reusable(c: &Connection) -> Result<bool> {
    // Body-only facts cannot change declarations. Derived global relationships and
    // merged/retargeted owners need their own refresh rules and keep the full path.
    Ok(c.query_row("SELECT
        NOT EXISTS(SELECT 1 FROM symbol_aliases)
        AND NOT EXISTS(SELECT 1 FROM facts_rows WHERE relation IN (
            SELECT id FROM atoms WHERE value IN ('inherits','implements','mixes_in','overrides','dispatch_candidate')))
        AND NOT EXISTS(SELECT 1 FROM bindings_rows WHERE kind IN (
            SELECT id FROM atoms WHERE value IN ('owner_hint','cxx_type')))
        AND NOT EXISTS(SELECT 1 FROM symbols_rows s JOIN symbols_rows t ON t.name_key=s.name_key AND t.id<>s.id
            WHERE s.language IN (SELECT id FROM atoms WHERE value IN ('c','cpp'))
              AND t.language IN (SELECT id FROM atoms WHERE value IN ('c','cpp')))", [], |r| r.get(0))?)
}
