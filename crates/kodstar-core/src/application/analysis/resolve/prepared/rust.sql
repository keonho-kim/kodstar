-- Purpose: Rust namespace candidate filtering.
-- Pattern: SQLite Query.
-- Usage: prepared.rs compiles this fixed statement once per generation.
-- Related: crates/kodstar-core/src/application/analysis/resolve/prepared.rs.

DELETE
FROM resolution_candidates
WHERE NOT EXISTS(
          SELECT 1
FROM symbols s
WHERE s.id=resolution_candidates.id
  AND CASE ?1
            WHEN 'macro' THEN s.kind_id IN (SELECT id FROM atoms WHERE value='macro')
            WHEN 'type' THEN s.kind_id IN (SELECT id FROM atoms WHERE value IN ('module','struct','enum','trait','type','class','interface'))
              OR EXISTS(SELECT 1
FROM bindings b
WHERE b.file_id=s.file_id
  AND b.target_local=s.local_id
  AND b.kind_id IN (SELECT id FROM atoms WHERE value='enum_case'))
            WHEN 'value' THEN s.kind_id IN (SELECT id FROM atoms WHERE value IN ('function','method','constructor'))
              OR (s.kind_id IN (SELECT id FROM atoms WHERE value IN ('struct','field'))
  AND (
                EXISTS(SELECT 1
FROM bindings b
WHERE b.file_id=s.file_id
  AND b.target_local=s.local_id
  AND b.kind_id IN (SELECT id FROM atoms WHERE value='rust_constructor'))
                OR (?2<>'calls'
  AND (s.kind_id IN (SELECT id FROM atoms WHERE value='field') OR EXISTS(SELECT 1
FROM bindings b
WHERE b.file_id=s.file_id
  AND b.target_local=s.local_id
  AND b.kind_id IN (SELECT id FROM atoms WHERE value='value_constructor'))))
              ))
            ELSE 1 END
           )
