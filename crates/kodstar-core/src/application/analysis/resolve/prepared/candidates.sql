-- Purpose: Binding target candidates.
-- Pattern: SQLite Query.
-- Usage: prepared.rs compiles this fixed statement once per generation.
-- Related: crates/kodstar-core/src/application/analysis/resolve/prepared.rs.

INSERT OR IGNORE INTO resolution_candidates SELECT s.id
FROM bindings b JOIN symbols s ON s.file_id=b.file_id
  AND s.local_id=b.target_local
WHERE b.file_id=?1
  AND b.scope_id=?2
  AND (b.name_key_id=(SELECT id FROM atoms WHERE value=?3) OR (?4='macro'
  AND b.name_key_id=(SELECT id FROM atoms WHERE value=?5)
  AND b.kind_id IN (SELECT id FROM atoms WHERE value IN ('import','export'))))
  AND (?4='any' OR b.namespace_id IN (SELECT id FROM atoms WHERE value IN ('any',?4)))
  AND b.target_local IS NOT NULL
