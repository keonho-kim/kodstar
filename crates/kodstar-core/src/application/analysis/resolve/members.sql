-- Purpose: Select nearest inherited members while deduplicating cycles on disk.
-- Pattern: Recursive Query.
-- Usage: member lookup binds the exact name and keyed-name flag.
-- Related: crates/kodstar-core/src/application/analysis/resolve/members.rs.

WITH RECURSIVE owners(root,id) AS (
                SELECT g.canonical_id,g.canonical_id FROM resolution_candidates c CROSS JOIN symbol_identities g ON g.symbol_id=c.id
                UNION
                SELECT o.root,r.target_id FROM owners o JOIN relationships r ON r.source_id=o.id
                JOIN facts f ON f.id=r.fact_id
                WHERE r.resolution='resolved' AND f.relation IN ('inherits','implements','mixes_in')
                  AND NOT EXISTS(SELECT 1 FROM symbols m JOIN symbol_identities g ON g.symbol_id=m.owner_id WHERE g.canonical_id=o.id AND CASE WHEN ?2 THEN m.name_key ELSE m.name END=?1)
             ) INSERT OR IGNORE INTO resolution_members
               SELECT m.id FROM owners o CROSS JOIN symbol_identities g ON g.canonical_id=o.id CROSS JOIN symbols m ON m.owner_id=g.symbol_id WHERE CASE WHEN ?2 THEN m.name_key ELSE m.name END=?1;
