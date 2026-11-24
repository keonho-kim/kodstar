-- Purpose: Select nearest inherited members while deduplicating cycles on disk.
-- Pattern: Recursive Query.
-- Usage: member lookup binds the exact name and keyed-name flag.
-- Related: crates/kodstar-core/src/application/analysis/resolve/members.rs.

WITH RECURSIVE owners(root,id) AS (
                SELECT COALESCE(a.canonical_id,s.id),COALESCE(a.canonical_id,s.id)
                FROM resolution_candidates c CROSS JOIN symbols_rows s ON s.id=c.id
                LEFT JOIN symbol_aliases a ON a.symbol_id=s.id
                UNION
                SELECT o.root,r.target_id FROM owners o CROSS JOIN relationships r ON r.source_id=o.id
                CROSS JOIN facts f ON f.id=r.fact_id
                WHERE r.resolution='resolved' AND f.relation IN ('inherits','implements','mixes_in')
                  AND NOT EXISTS(SELECT 1 FROM symbols m WHERE m.owner_id IN (
                      SELECT symbol_id FROM symbol_aliases WHERE canonical_id=o.id
                      UNION ALL SELECT o.id WHERE NOT EXISTS(SELECT 1 FROM symbol_aliases WHERE symbol_id=o.id)
                  ) AND CASE WHEN ?2 THEN m.name_key ELSE m.name END=?1)
             ) INSERT OR IGNORE INTO resolution_members
               SELECT m.id FROM owners o CROSS JOIN symbols m WHERE m.owner_id IN (
                   SELECT symbol_id FROM symbol_aliases WHERE canonical_id=o.id
                   UNION ALL SELECT o.id WHERE NOT EXISTS(SELECT 1 FROM symbol_aliases WHERE symbol_id=o.id)
               ) AND CASE WHEN ?2 THEN m.name_key ELSE m.name END=?1;
